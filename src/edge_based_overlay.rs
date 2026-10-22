//! Cells whose nodes are the arcs of a graph, worked out as they are asked
//! for.
//!
//! # What a cell holds
//!
//! An edge-based node is an arc, and the cell it lies in is the cell of the
//! node that arc runs *out of*. That choice is what makes a store of blocks
//! possible: a block names a border node by how far into the cell it sits, so
//! every id a table holds has to lie inside the cell, and a graph numbered so
//! that a cell's nodes run consecutively has that cell's arcs running
//! consecutively too. Keyed on the head instead, the ways out of a cell would
//! sit in the neighbouring one and no offset would name them.
//!
//! So a node of this overlay is "standing at the tail of this arc, about to
//! travel it", and what it cost to get there is the cost of everything before
//! it. A step across a cell runs from one such arc to another, and the arcs on
//! the border are the ones leaving a node that the cell's boundary touches.
//!
//! # Why the whole of it is not built
//!
//! [`crate::edge_based`] expands the graph as a search walks it. This holds
//! what a search over cells cannot work out on the fly: what it costs to cross
//! a cell. Those are tabulated once per cell, the first time a search asks.

use std::sync::OnceLock;

use log::debug;
use rustc_hash::FxHashMap;

use crate::{
    border_levels::BorderLevels,
    edge::InputEdge,
    edge_based::{EdgeBasedGraph, TurnCost},
    geometry::FPCoordinate,
    graph::{Adjacency, EdgeID, Graph, NodeID},
    level_directory::{CellId, LevelDirectory},
    one_to_many_dijkstra::OneToManyDijkstra,
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
    paged_overlay::PagedOverlay,
    static_graph::StaticGraph,
};

/// What it costs to cross a cell, from each way in to each way out.
///
/// # Why this is not square
///
/// The node-based table is: a cell's border nodes are both the ways in and the
/// ways out, so one list serves as rows and as columns. Arcs are not so
/// obliging. A route that has just entered a cell is standing on an arc leaving
/// the node it came in at, and there are as many of those as that node has
/// arcs; a route leaving the cell is on an arc that crosses the boundary, and
/// there is one of those per crossing. Measured over europe.ptv the first
/// outnumbers the second by 2.7 to one, so a square table of the wider set
/// holds three times the entries it needs.
pub struct ArcTable {
    /// the arcs a route may be standing on having just entered the cell
    rows: Vec<u32>,
    /// the arcs that cross out of the cell, which is what a step across it
    /// lands on
    columns: Vec<u32>,
    matrix: Vec<u32>,
    place_of: FxHashMap<NodeID, usize>,
}

impl ArcTable {
    fn of(rows: Vec<u32>, columns: Vec<u32>, matrix: Vec<u32>) -> Self {
        debug_assert_eq!(matrix.len(), rows.len() * columns.len(), "a table of rows");
        let place_of = rows
            .iter()
            .enumerate()
            .map(|(at, &arc)| (arc as NodeID, at))
            .collect();
        Self {
            rows,
            columns,
            matrix,
            place_of,
        }
    }
}

impl ArcTable {
    /// How many numbers the table holds.
    #[must_use]
    pub fn entries(&self) -> usize {
        self.matrix.len()
    }
}

impl CellTable for &ArcTable {
    /// The arcs a step across the cell lands on, which are its ways out.
    fn border_nodes(&self) -> &[u32] {
        &self.columns
    }

    fn row(&self, source: usize) -> &[u32] {
        let wide = self.columns.len();
        &self.matrix[source * wide..(source + 1) * wide]
    }

    /// Not held, and not wanted by any search that runs over this.
    ///
    /// A column is what a search running backwards through a cell reads, and
    /// this table is not square: its rows and its columns name different arcs,
    /// so the transpose is a different table rather than the same one read the
    /// other way. Nothing has needed it, so nothing has been built.
    fn column(&self, _target: usize) -> &[u32] {
        unimplemented!("an edge-based cell table holds no columns")
    }

    /// Where an arc sits among the ways in.
    fn place_of(&self, node: NodeID) -> Option<usize> {
        self.place_of.get(&node).copied()
    }
}

/// An overlay over the arcs of a graph rather than its nodes.
pub struct EdgeBasedOverlay<T: TurnCost> {
    graph: StaticGraph<u32>,
    coordinates: Vec<FPCoordinate>,
    turns: T,
    partition: PackedPartition,
    borders: BorderLevels,
    /// the node each arc runs out of, which every cell lookup asks for
    ///
    /// [`crate::edge_based`] works this out from the arc a search was standing
    /// on last. An overlay is asked which cells an arc lies in without being
    /// told how it was reached, so here it is written down: four bytes an arc,
    /// against tables that are already the larger part of what this holds.
    tail: Vec<u32>,
    /// where each cell's nodes begin, per level, the numbering being one where
    /// a cell's nodes run consecutively
    begins: Vec<Vec<u32>>,
    /// whether each node has the boundary of its cell running through it
    on_border: Vec<Vec<bool>>,
    /// whether each node is one an arc reaches from outside its cell, which is
    /// where a route entering the cell finds itself
    entered: Vec<Vec<bool>>,
    /// the cells of the level below that each cell is built out of, empty for
    /// the finest level, which is built out of the road network itself
    children: Vec<Vec<Vec<CellId>>>,
    tabulated: Vec<Vec<OnceLock<Option<Box<ArcTable>>>>>,
}

impl<T: TurnCost> EdgeBasedOverlay<T> {
    /// # Panics
    ///
    /// If the coordinates are not the graph's, or if the numbering is not one
    /// where each cell's nodes run consecutively. A cell's arcs are named by
    /// how far into the cell they sit, which is an answer only then.
    #[must_use]
    pub fn new(
        graph: StaticGraph<u32>,
        coordinates: Vec<FPCoordinate>,
        turns: T,
        directory: &LevelDirectory,
    ) -> Self {
        assert_eq!(
            coordinates.len(),
            Graph::number_of_nodes(&graph),
            "another set of coordinates"
        );
        let partition = PackedPartition::of(directory);
        let borders = BorderLevels::of(&graph, &partition);

        let mut tail = vec![0_u32; graph.number_of_edges()];
        for node in graph.node_range() {
            for arc in graph.edge_range(node) {
                tail[arc] = u32::try_from(node).expect("the graph is too large");
            }
        }

        let levels = directory.levels();
        let mut begins = Vec::with_capacity(levels);
        let mut on_border = Vec::with_capacity(levels);
        let mut entered = Vec::with_capacity(levels);
        for level in 0..levels {
            let cells = directory.cells_on_level(level);
            let mut at = vec![u32::MAX; cells + 1];
            let mut last = usize::MAX;
            for node in graph.node_range() {
                let cell = partition.cell_of(node, level) as usize;
                if cell == last {
                    continue;
                }
                assert!(
                    at[cell] == u32::MAX,
                    "the nodes of cell {cell} on level {level} do not run consecutively"
                );
                at[cell] = u32::try_from(node).expect("the graph is too large");
                last = cell;
            }
            // a cell nothing lies in begins where the next one does
            at[cells] =
                u32::try_from(Graph::number_of_nodes(&graph)).expect("the graph is too large");
            for cell in (0..cells).rev() {
                if at[cell] == u32::MAX {
                    at[cell] = at[cell + 1];
                }
            }

            // An arc across the boundary puts both of its ends on it: a node an
            // arc only reaches from outside is a way in, and a path across the
            // cell above may come by it.
            let mut touched = vec![false; Graph::number_of_nodes(&graph)];
            let mut reached = vec![false; Graph::number_of_nodes(&graph)];
            for node in graph.node_range() {
                for arc in graph.edge_range(node) {
                    if borders.leaves_cell(arc, level) {
                        touched[node] = true;
                        touched[graph.target(arc)] = true;
                        // the arc runs into the cell on the other side, so its
                        // head is where a route entering there stands
                        reached[graph.target(arc)] = true;
                    }
                }
            }
            begins.push(at);
            on_border.push(touched);
            entered.push(reached);
        }

        let mut children: Vec<Vec<Vec<CellId>>> = vec![Vec::new()];
        for level in 1..levels {
            let mut held = vec![Vec::new(); directory.cells_on_level(level)];
            for (child, &parent) in directory.parents_on_level(level - 1).iter().enumerate() {
                held[parent as usize]
                    .push(CellId::try_from(child).expect("more cells than a cell id counts"));
            }
            children.push(held);
        }

        let tabulated = (0..levels)
            .map(|level| {
                (0..directory.cells_on_level(level))
                    .map(|_| OnceLock::new())
                    .collect()
            })
            .collect();

        Self {
            graph,
            coordinates,
            turns,
            partition,
            borders,
            tail,
            begins,
            on_border,
            entered,
            children,
            tabulated,
        }
    }

    /// The node an arc runs out of.
    #[must_use]
    pub fn tail(&self, arc: EdgeID) -> NodeID {
        self.tail[arc] as NodeID
    }

    /// What the arcs of a cell count from, per level and cell.
    ///
    /// A cell's nodes run consecutively and its arcs are grouped by the node
    /// they run out of, so its arcs run consecutively too. That is what lets a
    /// table of arcs be written into a block, whose ids are offsets.
    #[must_use]
    pub fn arcs_begin(&self) -> Vec<Vec<u32>> {
        (0..self.begins.len())
            .map(|level| {
                (0..self.cells_on_level(level))
                    .map(|cell| {
                        let nodes = self.nodes_of(level, cell as CellId);
                        u32::try_from(self.graph.edge_range(nodes.start).start)
                            .expect("the graph is too large")
                    })
                    .collect()
            })
            .collect()
    }

    /// How wide each cell's table is, per level and cell, which a store has no
    /// other way of knowing: it reads a width off the tree, and the tree counts
    /// the nodes on a cell's border rather than the arcs.
    ///
    /// # Panics
    ///
    /// If a cell has not been tabulated yet, since a width nobody has worked
    /// out is not a width.
    #[must_use]
    pub fn border_widths(&self) -> Vec<Vec<u32>> {
        (0..self.begins.len())
            .map(|level| {
                (0..self.cells_on_level(level))
                    .map(|cell| {
                        self.distances_of(level, cell as CellId).map_or(0, |table| {
                            u32::try_from(table.columns.len()).expect("a cell wider than a u32")
                        })
                    })
                    .collect()
            })
            .collect()
    }

    /// How many arcs each cell holds, per level and cell, which is what a place
    /// written into a block needs room for.
    #[must_use]
    pub fn arcs_held(&self) -> Vec<Vec<usize>> {
        (0..self.begins.len())
            .map(|level| {
                (0..self.cells_on_level(level))
                    .map(|cell| {
                        let nodes = self.nodes_of(level, cell as CellId);
                        if nodes.is_empty() {
                            return 0;
                        }
                        self.graph.edge_range(nodes.end - 1).end
                            - self.graph.edge_range(nodes.start).start
                    })
                    .collect()
            })
            .collect()
    }

    /// The nodes of a cell, which run consecutively.
    fn nodes_of(&self, level: usize, cell: CellId) -> std::ops::Range<NodeID> {
        let at = &self.begins[level];
        at[cell as usize] as NodeID..at[cell as usize + 1] as NodeID
    }

    /// The ways into a cell and the ways out of it.
    ///
    /// A route that has just entered is standing on an arc leaving the node it
    /// came in at, so the ways in are every arc of every node an arc reaches
    /// from outside. A route leaving is on an arc that crosses the boundary, so
    /// the ways out are those and no more. The second set is the smaller by
    /// nearly three to one, which is why they are counted apart.
    fn ways_in_and_out(&self, level: usize, cell: CellId) -> (Vec<u32>, Vec<u32>) {
        let inside = self.nodes_of(level, cell);
        let (mut rows, mut columns) = (Vec::new(), Vec::new());
        for node in inside.clone() {
            if self.on_border[level][node] {
                for arc in self.graph.edge_range(node) {
                    if !inside.contains(&self.graph.target(arc)) {
                        columns.push(u32::try_from(arc).expect("the graph is too large"));
                    }
                }
            }
            if self.entered[level][node] {
                for arc in self.graph.edge_range(node) {
                    rows.push(u32::try_from(arc).expect("the graph is too large"));
                }
            }
        }
        (rows, columns)
    }

    /// The turns inside a cell, as a graph over the arcs of it.
    ///
    /// The ways out lead the numbering, so that a search run as far as them
    /// answers a whole row of the table. An arc is a node here, and a turn from
    /// one to the next costs what travelling the first costs plus what the turn
    /// does, so that what a row says is the cost of reaching the tail of the
    /// arc it names.
    fn turn_graph(&self, level: usize, cell: CellId, numbering: &[u32]) -> StaticGraph<u32> {
        let mut of_arc: FxHashMap<EdgeID, NodeID> = FxHashMap::default();
        for &arc in numbering {
            let next = of_arc.len();
            of_arc.insert(arc as EdgeID, next);
        }
        let inside = self.nodes_of(level, cell);
        let expanded = EdgeBasedGraph::new(&self.graph, &self.coordinates, &self.turns);

        let mut edges = Vec::new();
        for node in inside.clone() {
            for arc in self.graph.edge_range(node) {
                let head = self.graph.target(arc);
                if !inside.contains(&head) {
                    // the arc leaves the cell, so nothing it turns onto is in
                    continue;
                }
                let travelled = *self.graph.data(arc);
                expanded.for_each_turn_cost(arc, node, |onto, turn| {
                    let next = of_arc.len();
                    let source = *of_arc.entry(arc).or_insert(next);
                    let next = of_arc.len();
                    let target = *of_arc.entry(onto).or_insert(next);
                    edges.push(InputEdge::new(source, target, travelled + turn));
                });
            }
        }
        let nodes = of_arc.len().max(numbering.len());
        edges.sort_unstable();
        StaticGraph::from_sorted_slice(nodes, &edges)
    }

    /// The turns inside a cell, as a graph over the border arcs of the cells it
    /// is built out of.
    ///
    /// A coarse cell holds too much of the graph to search arc by arc. What a
    /// route does inside a cell below is already tabulated, and what it does
    /// between two of them is a turn of the road network, so those are what is
    /// searched instead.
    fn turn_graph_of_children(
        &self,
        level: usize,
        cell: CellId,
        border: &[u32],
    ) -> StaticGraph<u32> {
        let mut of_arc: FxHashMap<EdgeID, NodeID> = FxHashMap::default();
        for &arc in border {
            let next = of_arc.len();
            of_arc.insert(arc as EdgeID, next);
        }
        let expanded = EdgeBasedGraph::new(&self.graph, &self.coordinates, &self.turns);
        let mut edges = Vec::new();

        for &child in &self.children[level][cell as usize] {
            let Some(across) = self.distances_of(level - 1, child) else {
                // a cell below with no border cannot be entered or left, so no
                // route of this cell runs through it
                continue;
            };

            // what a route does inside the cell below, which is tabulated
            let wide = across.columns.len();
            for (source, &from) in across.rows.iter().enumerate() {
                for (target, &to) in across.columns.iter().enumerate() {
                    let weight = across.matrix[source * wide + target];
                    if from == to || weight == u32::MAX {
                        continue;
                    }
                    let next = of_arc.len();
                    let from = *of_arc.entry(from as EdgeID).or_insert(next);
                    let next = of_arc.len();
                    let to = *of_arc.entry(to as EdgeID).or_insert(next);
                    edges.push(InputEdge::new(from, to, weight));
                }
            }

            // and what it does crossing from one cell below into another one of
            // this cell, which is a turn of the road network
            for &arc in &across.columns {
                let arc = arc as EdgeID;
                let head = self.graph.target(arc);
                if self.partition.cell_of(head, level) != cell
                    || self.partition.cell_of(head, level - 1) == child
                {
                    continue;
                }
                let travelled = *self.graph.data(arc);
                expanded.for_each_turn_cost(arc, self.tail(arc), |onto, turn| {
                    let next = of_arc.len();
                    let from = *of_arc.entry(arc).or_insert(next);
                    let next = of_arc.len();
                    let to = *of_arc.entry(onto).or_insert(next);
                    edges.push(InputEdge::new(from, to, travelled + turn));
                });
            }
        }

        let nodes = of_arc.len().max(border.len());
        edges.sort_unstable();
        StaticGraph::from_sorted_slice(nodes, &edges)
    }

    /// What it costs to cross a cell, worked out the first time it is asked
    /// for.
    fn tabulate(&self, level: usize, cell: CellId) -> Option<ArcTable> {
        let (rows, columns) = self.ways_in_and_out(level, cell);
        if rows.is_empty() || columns.is_empty() {
            debug!("cell {cell} of level {level} has no way in or none out");
            return None;
        }

        // the ways out lead, so that a search as far as them is a whole row
        let mut numbering = columns.clone();
        numbering.extend(rows.iter().filter(|arc| !columns.contains(arc)));
        let turns = if level == 0 {
            self.turn_graph(level, cell, &numbering)
        } else {
            self.turn_graph_of_children(level, cell, &numbering)
        };
        let of_arc: FxHashMap<u32, usize> = numbering
            .iter()
            .enumerate()
            .map(|(at, &arc)| (arc, at))
            .collect();

        let wide = columns.len();
        let mut matrix = vec![u32::MAX; rows.len() * wide];
        let mut search = OneToManyDijkstra::new();
        for (source, arc) in rows.iter().enumerate() {
            let from = of_arc[arc];
            search.run_to_leading(&turns, from, wide);
            for (target, across) in matrix[source * wide..(source + 1) * wide]
                .iter_mut()
                .enumerate()
            {
                *across = u32::try_from(search.distance(target)).unwrap_or(u32::MAX);
            }
        }
        Some(ArcTable::of(rows, columns, matrix))
    }
}

/// The same arcs a search over the cells walks near its ends, so that a plain
/// search and one over the cells can be held against each other over exactly
/// the same graph, the same turns and the same prices.
impl<T: TurnCost> Adjacency<u32> for EdgeBasedOverlay<T> {
    fn number_of_nodes(&self) -> usize {
        self.graph.number_of_edges()
    }

    fn for_each_arc(&self, n: NodeID, from: NodeID, f: impl FnMut(NodeID, u32)) {
        Overlay::for_each_arc(self, n, from, f);
    }
}

impl<T: TurnCost> Overlay for EdgeBasedOverlay<T> {
    type Graph = StaticGraph<u32>;
    type Table<'a>
        = &'a ArcTable
    where
        Self: 'a;
    type Borders = BorderLevels;

    fn graph(&self) -> &Self::Graph {
        &self.graph
    }

    fn partition(&self) -> &PackedPartition {
        &self.partition
    }

    fn borders(&self) -> &Self::Borders {
        &self.borders
    }

    fn levels(&self) -> usize {
        self.begins.len()
    }

    fn cells_on_level(&self, level: usize) -> usize {
        self.begins[level].len() - 1
    }

    /// An arc lies in the cells of the node it runs out of.
    fn word_of(&self, node: NodeID) -> u128 {
        self.partition.word(self.tail(node))
    }

    /// The turns out of an arc, each costing what travelling the arc costs and
    /// what the turn itself does.
    fn for_each_arc(&self, node: NodeID, from: NodeID, mut f: impl FnMut(NodeID, u32)) {
        let _ = from;
        let travelled = *self.graph.data(node);
        let expanded = EdgeBasedGraph::new(&self.graph, &self.coordinates, &self.turns);
        expanded.for_each_turn_cost(node, self.tail(node), |onto, turn| {
            f(onto, travelled + turn);
        });
    }

    /// Whether the turns out of an arc leave its cell is a fact about the arc
    /// and not about the turn: every turn out of it starts where it ends, so
    /// either all of them leave or none of them do.
    fn for_each_arc_out_of_cell(
        &self,
        node: NodeID,
        from: NodeID,
        level: usize,
        f: impl FnMut(NodeID, u32),
    ) {
        if self.borders.leaves_cell(node, level) {
            Overlay::for_each_arc(self, node, from, f);
        }
    }

    fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>> {
        let slot = self.tabulated.get(level)?.get(cell as usize)?;
        if let Some(held) = slot.get() {
            return held.as_deref();
        }
        let _ = slot.set(self.tabulate(level, cell).map(Box::new));
        slot.get().and_then(Option::as_deref)
    }
}

/// The same cells, read off a file rather than held.
///
/// The turns are worked out as a search walks them, exactly as they are held in
/// memory. What differs is only where the tables come from: a
/// [`PagedOverlay`](crate::paged_overlay::PagedOverlay) reads and decompresses
/// a block when one is asked for, and throws a table away when the room is
/// wanted. Everything about the arcs is answered here.
pub struct PagedEdgeBasedOverlay<T: TurnCost> {
    tables: PagedOverlay<StaticGraph<u32>, BorderLevels>,
    graph: StaticGraph<u32>,
    coordinates: Vec<FPCoordinate>,
    turns: T,
    tail: Vec<u32>,
}

impl<T: TurnCost> PagedEdgeBasedOverlay<T> {
    #[must_use]
    pub fn new(
        tables: PagedOverlay<StaticGraph<u32>, BorderLevels>,
        graph: StaticGraph<u32>,
        coordinates: Vec<FPCoordinate>,
        turns: T,
    ) -> Self {
        let mut tail = vec![0_u32; graph.number_of_edges()];
        for node in graph.node_range() {
            for arc in graph.edge_range(node) {
                tail[arc] = u32::try_from(node).expect("the graph is too large");
            }
        }
        Self {
            tables,
            graph,
            coordinates,
            turns,
            tail,
        }
    }

    /// The node an arc runs out of.
    #[must_use]
    pub fn tail(&self, arc: EdgeID) -> NodeID {
        self.tail[arc] as NodeID
    }

    /// What the pool did, which says whether a run really read off the file.
    #[must_use]
    pub fn faults(&self) -> crate::paged_overlay::Faults {
        self.tables.faults()
    }
}

impl<T: TurnCost> Overlay for PagedEdgeBasedOverlay<T> {
    type Graph = StaticGraph<u32>;
    type Table<'a>
        = <PagedOverlay<StaticGraph<u32>, BorderLevels> as Overlay>::Table<'a>
    where
        Self: 'a;
    type Borders = BorderLevels;

    fn graph(&self) -> &Self::Graph {
        &self.graph
    }

    fn partition(&self) -> &PackedPartition {
        self.tables.partition()
    }

    fn borders(&self) -> &Self::Borders {
        self.tables.borders()
    }

    fn levels(&self) -> usize {
        self.tables.levels()
    }

    fn cells_on_level(&self, level: usize) -> usize {
        self.tables.cells_on_level(level)
    }

    fn word_of(&self, node: NodeID) -> u128 {
        self.partition().word(self.tail(node))
    }

    fn for_each_arc(&self, node: NodeID, from: NodeID, mut f: impl FnMut(NodeID, u32)) {
        let _ = from;
        let travelled = *self.graph.data(node);
        let expanded = EdgeBasedGraph::new(&self.graph, &self.coordinates, &self.turns);
        expanded.for_each_turn_cost(node, self.tail(node), |onto, turn| {
            f(onto, travelled + turn);
        });
    }

    fn for_each_arc_out_of_cell(
        &self,
        node: NodeID,
        from: NodeID,
        level: usize,
        f: impl FnMut(NodeID, u32),
    ) {
        if self.borders().leaves_cell(node, level) {
            Overlay::for_each_arc(self, node, from, f);
        }
    }

    fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>> {
        self.tables.distances_of(level, cell)
    }
}

#[cfg(test)]
mod tests {
    use super::EdgeBasedOverlay;
    use crate::{
        cell_ordering::CellOrdering,
        edge::InputEdge,
        edge_based::{AnglePenalty, FreeTurns, NoUTurns, TurnCost},
        geometry::FPCoordinate,
        graph::{Graph, NodeID},
        grid_graph::grid_directory,
        level_directory::LevelDirectory,
        mld_query::MldQuery,
        node_ordering::{NodeOrdering, Numbering},
        packed_partition::PackedPartition,
        static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    fn grid_edges(side: usize) -> Vec<InputEdge<u32>> {
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                let weight = (1 + (row * 7 + column * 3) % 9) as u32;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, weight));
                    edges.push(InputEdge::new(node + 1, node, weight));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, weight + 1));
                    edges.push(InputEdge::new(node + side, node, weight + 1));
                }
            }
        }
        edges
    }

    /// A grid whose nodes are numbered so that each cell holds a run of them,
    /// which is what a table of arcs named by their place in a cell wants.
    fn laid_out(side: usize) -> (Vec<InputEdge<u32>>, LevelDirectory, Vec<FPCoordinate>) {
        let edges = grid_edges(side);
        let directory = grid_directory(side);
        let directory =
            CellOrdering::of(&directory, &PackedPartition::of(&directory)).renumber(&directory);
        let graph = StaticGraph::new(edges.clone());
        let ordering = NodeOrdering::in_order(
            &graph,
            &PackedPartition::of(&directory),
            Numbering::CellPath,
        );
        // laid out on the ground the way the grid is, so that a turn has an
        // angle worth pricing
        let mut coordinates = vec![FPCoordinate::new(0, 0); side * side];
        for row in 0..side {
            for column in 0..side {
                let at = ordering.new_of(row * side + column);
                coordinates[at] = FPCoordinate::new(
                    i32::try_from(row * 1000).unwrap(),
                    i32::try_from(column * 1000).unwrap(),
                );
            }
        }
        (
            ordering.renumber(&edges),
            ordering.renumber_directory(&directory),
            coordinates,
        )
    }

    /// The one that matters: the same turns, priced the same way, answered by a
    /// search over the cells and by a plain search over the whole graph.
    fn agrees_with_a_plain_search<T: TurnCost + Clone>(turns: T) {
        let side = 16;
        let (edges, directory, coordinates) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let overlay = EdgeBasedOverlay::new(
            StaticGraph::new(edges.clone()),
            coordinates.clone(),
            turns.clone(),
            &directory,
        );

        let mut over_cells = MldQuery::new();
        let mut plain = UnidirectionalDijkstra::new();
        let mut pairs = 0;
        for source in (0..side * side).step_by(5) {
            for target in (0..side * side).step_by(7) {
                if source == target {
                    continue;
                }
                // A query is between nodes and this searches over arcs, so
                // it starts on each arc leaving the source and finishes on the
                // arcs leaving the target: standing on one of those is
                // standing at the target, which is what its distance says.
                let targets: Vec<NodeID> = graph.edge_range(target).collect();
                let mut over_cells_said = usize::MAX;
                for arc in graph.edge_range(source) {
                    over_cells.run(&overlay, arc, &targets);
                    for &at in &targets {
                        over_cells_said = over_cells_said.min(over_cells.distance(at));
                    }
                }
                // the same arcs, the same turns and the same prices, walked
                // one at a time instead of stepped over: what the cells say has
                // to be what this says
                let sources: Vec<(NodeID, usize)> =
                    graph.edge_range(source).map(|arc| (arc, 0)).collect();
                plain.run_many(&overlay, &sources, |_| false);
                let mut plain_said = usize::MAX;
                for &at in &targets {
                    plain_said = plain_said.min(plain.distance(at));
                }
                assert_eq!(over_cells_said, plain_said, "from {source} to {target}");
                pairs += 1;
            }
        }
        assert!(pairs > 500, "the sweep is worth running");
    }

    #[test]
    fn free_turns_over_cells_answer_what_a_plain_search_does() {
        agrees_with_a_plain_search(FreeTurns);
    }

    #[test]
    fn refused_reversals_over_cells_answer_what_a_plain_search_does() {
        agrees_with_a_plain_search(NoUTurns);
    }

    #[test]
    fn priced_angles_over_cells_answer_what_a_plain_search_does() {
        agrees_with_a_plain_search(AnglePenalty::new(30., 100));
    }
}
