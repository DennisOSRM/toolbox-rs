//! The cells of a partitioned graph, level by level.
//!
//! A partition on its own says which cell each node sits in. What a caller
//! wants is the other way round as well: the nodes of each cell, which of them
//! sit on a border, and which cells of the level below each cell is built out
//! of.
//! Working that out means a walk of the whole graph, so it is done once per
//! level and kept.
//!
//! # Border nodes
//!
//! A node is on the border of its cell while an arc leaves it or reaches it
//! from outside. Both count. A road network is directed, and a node that can
//! only be entered from another cell is still a way into the cell, which a
//! path through the level above may take.

use crate::{
    edge::InputEdge,
    graph::{Graph, NodeID},
    level_directory::{CellId, LevelDirectory},
    one_to_many_dijkstra::OneToManyDijkstra,
    static_graph::StaticGraph,
};
use log::debug;
use rustc_hash::FxHashMap;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The distances between the border nodes of one cell, in the order the border
/// nodes are listed in.
pub struct CellDistances {
    pub border_nodes: Vec<NodeID>,
    matrix: Vec<usize>,
}

impl CellDistances {
    /// What it costs to get from one border node of the cell to another,
    /// both given as their place in `border_nodes`.
    pub fn distance(&self, source: usize, target: usize) -> usize {
        self.matrix[source * self.border_nodes.len() + target]
    }
}

/// The cells of one level: which one each node sits in, which nodes each of
/// them holds, and which of those nodes sit on a border.
pub struct Level {
    pub of_node: Vec<CellId>,
    pub nodes_of_cell: Vec<Vec<NodeID>>,
    /// A node is on the border of its cell while an arc leaves it or reaches it
    /// from outside. Both count: a road network is directed, and a node that
    /// can only be entered from another cell is a way in that a path through
    /// the cell above may take.
    pub on_border: Vec<bool>,
    /// the cells of the level below that each cell of this one is built out of,
    /// and empty on the finest level, which is built from the graph itself
    pub built_from: Vec<Vec<CellId>>,
}

/// The cells of a partition, worked out level by level as they are asked for.
pub struct Customization {
    graph: StaticGraph<usize>,
    directory: LevelDirectory,
    /// The cells of a level, and the nodes of each of them, worked out the
    /// first time that level is asked about. Walking the directory per node
    /// per arc would otherwise be paid on every request.
    levels: Mutex<FxHashMap<usize, Arc<Level>>>,
    /// A cell is customized the first time it is asked about and kept
    /// afterwards. Doing it up front would mean walking every cell of the
    /// input before the first request can be answered.
    tabulated: Mutex<FxHashMap<(usize, CellId), Arc<CellDistances>>>,
    /// how many cells have been customized so far, and how long that took in
    /// total. The customization runs cell by cell as the cells are asked
    /// about, so the sum is what the whole of it would have cost up front.
    customized_cells: AtomicUsize,
    customization_nanos: AtomicU64,
}

impl Customization {
    #[must_use]
    pub fn new(graph: StaticGraph<usize>, directory: LevelDirectory) -> Self {
        assert_eq!(
            graph.number_of_nodes(),
            directory.number_of_nodes(),
            "the directory was built over another graph"
        );
        Self {
            graph,
            directory,
            levels: Mutex::new(FxHashMap::default()),
            tabulated: Mutex::new(FxHashMap::default()),
            customized_cells: AtomicUsize::new(0),
            customization_nanos: AtomicU64::new(0),
        }
    }

    /// the graph the partition was built over
    pub const fn graph(&self) -> &StaticGraph<usize> {
        &self.graph
    }

    /// which cell each node sits in on each level
    pub const fn directory(&self) -> &LevelDirectory {
        &self.directory
    }

    /// how many cells have been worked out so far
    pub fn customized_cells(&self) -> usize {
        self.customized_cells.load(Ordering::Relaxed)
    }

    /// what all of them together have cost, summed over whatever threads did
    /// the work rather than measured on the clock on the wall
    pub fn customization_time(&self) -> Duration {
        Duration::from_nanos(self.customization_nanos.load(Ordering::Relaxed))
    }

    /// Drops the distances worked out so far, for a caller that is done with
    /// them. The cells of a level are kept, as they cost a walk of the whole
    /// graph and take no room per cell.
    pub fn forget(&self) {
        self.tabulated
            .lock()
            .expect("the tabulation cache is poisoned")
            .clear();
    }

    /// The cells of a level, worked out on the first request for it and kept.
    pub fn level(&self, level: usize) -> Arc<Level> {
        if let Some(cells) = self
            .levels
            .lock()
            .expect("the level cache is poisoned")
            .get(&level)
        {
            return cells.clone();
        }

        let of_node = (0..self.directory.number_of_nodes())
            .map(|node| self.directory.cell_of(node, level))
            .collect::<Vec<_>>();
        let mut nodes_of_cell = vec![Vec::new(); self.directory.cells_on_level(level)];
        for (node, &cell) in of_node.iter().enumerate() {
            nodes_of_cell[cell as usize].push(node);
        }

        // one walk of the arcs marks both ends of every arc that leaves a cell,
        // which saves holding the arcs of the graph the other way round
        let mut on_border = vec![false; of_node.len()];
        for source in self.graph.node_range() {
            for edge in self.graph.edge_range(source) {
                let target = self.graph.target(edge);
                if of_node[source] != of_node[target] {
                    on_border[source] = true;
                    on_border[target] = true;
                }
            }
        }

        let built_from = if level == 0 {
            Vec::new()
        } else {
            let mut children = vec![Vec::new(); self.directory.cells_on_level(level)];
            for (below, &above) in self
                .directory
                .parents_on_level(level - 1)
                .iter()
                .enumerate()
            {
                children[above as usize].push(below as CellId);
            }
            children
        };

        let cells = Arc::new(Level {
            of_node,
            nodes_of_cell,
            on_border,
            built_from,
        });
        // another thread may have worked the same level out while this one
        // was busy, and whichever entry got there first is kept, so that a
        // level is one object however many threads asked for it
        self.levels
            .lock()
            .expect("the level cache is poisoned")
            .entry(level)
            .or_insert(cells)
            .clone()
    }

    /// Hands out the distances of a cell, tabulating them on the first request.
    pub fn distances_of(&self, level: usize, cell: CellId) -> Option<Arc<CellDistances>> {
        if let Some(distances) = self
            .tabulated
            .lock()
            .expect("the tabulation cache is poisoned")
            .get(&(level, cell))
        {
            return Some(distances.clone());
        }

        // as with the levels, the first entry to land is the one that is kept.
        // Two callers asking for the same cell at once both work it out, and
        // the tally counts both, as both were really paid for.
        let distances = Arc::new(self.tabulate(level, cell)?);
        Some(
            self.tabulated
                .lock()
                .expect("the tabulation cache is poisoned")
                .entry((level, cell))
                .or_insert(distances)
                .clone(),
        )
    }

    /// Builds the graph of a cell and runs a search from each of its border
    /// nodes. A cell is a small part of the input, so this is quick enough to
    /// happen while a caller waits for it.
    fn tabulate(&self, level: usize, cell: CellId) -> Option<CellDistances> {
        let started = Instant::now();
        let cells = self.level(level);
        let nodes = cells.nodes_of_cell.get(cell as usize)?;

        // the border nodes lead the numbering, so that they are the leading
        // rows and columns of the matrix
        let border_nodes = nodes
            .iter()
            .copied()
            .filter(|&node| cells.on_border[node])
            .collect::<Vec<_>>();
        if border_nodes.is_empty() {
            debug!("cell {cell} of level {level} has no border nodes");
            return None;
        }

        let cell_graph = self.subgraph_of(&cells, cell, nodes, &border_nodes);

        // the border nodes lead the numbering of that graph too
        let border = (0..border_nodes.len()).collect::<Vec<_>>();
        let mut matrix = vec![usize::MAX; border_nodes.len() * border_nodes.len()];
        let mut dijkstra = OneToManyDijkstra::new();
        for &source in &border {
            dijkstra.run(&cell_graph, source, &border);
            for &target in &border {
                matrix[source * border_nodes.len() + target] = dijkstra.distance(target);
            }
        }
        // the searches are what the customization of a cell costs, so the
        // clock is read once they are done
        self.customized_cells.fetch_add(1, Ordering::Relaxed);
        self.customization_nanos
            .fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);

        Some(CellDistances {
            border_nodes,
            matrix,
        })
    }

    /// The arcs of the graph that stay inside a cell, with its border nodes
    /// numbered first. This is what the finest level is built from, as there is
    /// no level below it to take distances from.
    fn subgraph_of(
        &self,
        cells: &Level,
        cell: CellId,
        nodes: &[NodeID],
        border_nodes: &[NodeID],
    ) -> StaticGraph<usize> {
        // TODO: faster hashmap implementation using tabhash or fibonacci hash
        let mut of_node = FxHashMap::default();
        for &node in border_nodes {
            of_node.insert(node, of_node.len());
        }
        let mut edges = Vec::new();
        for &node in nodes {
            for edge in self.graph.edge_range(node) {
                let target = self.graph.target(edge);
                if cells.of_node[target] != cell {
                    continue;
                }
                let next = of_node.len();
                let source = *of_node.entry(node).or_insert(next);
                let next = of_node.len();
                let target = *of_node.entry(target).or_insert(next);
                edges.push(InputEdge::new(source, target, *self.graph.data(edge)));
            }
        }
        // A border node whose arcs all leave the cell has none inside it and so
        // appears in no arc here. The graph is asked for the nodes the cell
        // has all the same, or a search started from that node would read past
        // the end of it.
        // TODO: find a way to avoid relocations
        StaticGraph::new_with_nodes(of_node.len().max(border_nodes.len()), edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square grid of `side` by `side` nodes, cut into squares of two by two
    /// on the finest level and of four by four above it. The arcs of a row run
    /// one way round when `both_ways` is not asked for, which is what a road
    /// network does and what makes the distances of a cell asymmetric.
    /// Two cells of two nodes each, joined on the level above.
    fn two_cells() -> Customization {
        let edges = vec![
            InputEdge::new(0, 1, 3_usize),
            InputEdge::new(1, 0, 3_usize),
            InputEdge::new(1, 2, 7_usize),
            InputEdge::new(2, 1, 7_usize),
            InputEdge::new(2, 3, 5_usize),
            InputEdge::new(3, 2, 5_usize),
        ];
        // nodes 0 and 1 in one cell, nodes 2 and 3 in the other, joined above
        let directory = LevelDirectory::new(vec![0, 0, 1, 1], vec![vec![0, 0]]);
        Customization::new(StaticGraph::new(edges), directory)
    }

    /// A square grid of `side` by `side` nodes, cut into squares of two by two
    /// on the finest level and of four by four above it. The arcs of a row run
    /// one way round when `both_ways` is not asked for, which is what a road
    /// network does and what makes the distances of a cell asymmetric.
    fn grid_with(side: usize, both_ways: bool) -> Customization {
        let node = |row: usize, column: usize| row * side + column;
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                if column + 1 < side {
                    edges.push(InputEdge::new(node(row, column), node(row, column + 1), 1));
                    if both_ways {
                        edges.push(InputEdge::new(node(row, column + 1), node(row, column), 1));
                    }
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node(row, column), node(row + 1, column), 1));
                    edges.push(InputEdge::new(node(row + 1, column), node(row, column), 1));
                }
            }
        }

        let finest = (0..side * side)
            .map(|index| {
                let (row, column) = (index / side, index % side);
                ((row / 2) * (side / 2) + column / 2) as CellId
            })
            .collect::<Vec<_>>();
        let coarser = (0..(side / 2) * (side / 2))
            .map(|cell| {
                let (row, column) = (cell / (side / 2), cell % (side / 2));
                ((row / 2) * (side / 4) + column / 2) as CellId
            })
            .collect::<Vec<_>>();
        let top = vec![0; (side / 4) * (side / 4)];

        Customization::new(
            StaticGraph::new(edges),
            LevelDirectory::new(finest, vec![coarser, top]),
        )
    }

    fn grid(side: usize) -> Customization {
        grid_with(side, true)
    }

    #[test]
    fn a_border_node_is_one_an_arc_reaches_as_well_as_one_it_leaves() {
        // 0 -> 1 only, and the two sit in different cells. Node 1 can only be
        // entered from outside, and is a way into its cell all the same.
        let edges = vec![InputEdge::new(0, 1, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 1], vec![vec![0, 0]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);

        let cells = customization.level(0);
        assert!(cells.on_border[0], "the node the arc leaves");
        assert!(cells.on_border[1], "the node the arc reaches");
    }

    #[test]
    fn a_cell_knows_the_cells_it_is_built_from() {
        let customization = grid(8);
        let cells = customization.level(1);
        // four cells of the finest level make up one of the level above
        for children in &cells.built_from {
            assert_eq!(children.len(), 4);
        }
        // and the finest level is built from the graph rather than from cells
        assert!(customization.level(0).built_from.is_empty());
    }

    #[test]
    fn distances_within_a_cell_are_tabulated_on_request() {
        let customization = two_cells();
        let distances = customization
            .distances_of(0, 0)
            .expect("cell 0 has a border");

        // node 1 is the only border node of its cell, so the matrix is 1x1 and
        // the distance to itself is zero
        assert_eq!(distances.border_nodes, vec![1]);
        assert_eq!(distances.distance(0, 0), 0);
    }

    #[test]
    fn a_tabulated_cell_is_kept() {
        let customization = two_cells();
        let first = customization.distances_of(0, 0).expect("no cell 0");
        let second = customization.distances_of(0, 0).expect("no cell 0");
        // the second request is answered from the same tabulation
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn what_was_forgotten_is_worked_out_again() {
        let customization = two_cells();
        let first = customization.distances_of(0, 0).expect("no cell 0");
        customization.forget();
        let second = customization.distances_of(0, 0).expect("no cell 0");

        assert!(!Arc::ptr_eq(&first, &second), "the cell was kept");
        assert_eq!(first.border_nodes, second.border_nodes);
        assert_eq!(customization.customized_cells(), 2);
    }

    /// A border node whose arcs all leave its cell has none inside it, so it
    /// turns up in no arc of the subgraph. The graph still has to hold it, or
    /// a search started there reads past the end of the node array.
    #[test]
    fn a_cell_whose_arcs_all_leave_it_is_still_tabulated() {
        // nodes 0 and 1 sit in one cell and are joined only through node 2,
        // which sits in another, so the first cell holds no arc at all
        let edges = vec![
            InputEdge::new(0, 2, 1_usize),
            InputEdge::new(2, 0, 1_usize),
            InputEdge::new(1, 2, 1_usize),
            InputEdge::new(2, 1, 1_usize),
        ];
        let directory = LevelDirectory::new(vec![0, 0, 1], vec![vec![0, 0]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);

        let distances = customization
            .distances_of(0, 0)
            .expect("both are border nodes");
        assert_eq!(distances.border_nodes, vec![0, 1]);
        // each reaches itself and neither reaches the other without leaving
        assert_eq!(distances.distance(0, 0), 0);
        assert_eq!(distances.distance(1, 1), 0);
        assert_eq!(distances.distance(0, 1), usize::MAX);
        assert_eq!(distances.distance(1, 0), usize::MAX);
    }

    #[test]
    fn a_cell_without_a_border_is_not_tabulated() {
        // one cell holding the whole graph, so no arc ever leaves it
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 0], Vec::new());
        let customization = Customization::new(StaticGraph::new(edges), directory);

        assert!(customization.distances_of(0, 0).is_none());
    }

    #[test]
    fn customization_is_counted_once_per_cell() {
        let customization = two_cells();
        assert_eq!(customization.customized_cells(), 0);

        customization.distances_of(0, 0).expect("no cell 0");
        assert_eq!(customization.customized_cells(), 1);
        assert!(customization.customization_time() > Duration::ZERO);

        // the second cell adds to the tally
        customization.distances_of(0, 1).expect("no cell 1");
        assert_eq!(customization.customized_cells(), 2);

        // a cell that is answered from the tabulation of an earlier request
        // was not customized again
        let after = customization.customization_time();
        customization.distances_of(0, 0).expect("no cell 0");
        assert_eq!(customization.customized_cells(), 2);
        assert_eq!(customization.customization_time(), after);
    }

    #[test]
    fn a_level_is_worked_out_once_and_kept() {
        let customization = grid(8);
        let first = customization.level(1);
        let second = customization.level(1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_cell_holds_the_nodes_the_directory_puts_in_it() {
        let customization = grid(8);
        let cells = customization.level(0);
        assert_eq!(cells.nodes_of_cell.len(), 16, "squares of two by two");
        for (cell, nodes) in cells.nodes_of_cell.iter().enumerate() {
            assert_eq!(nodes.len(), 4);
            for &node in nodes {
                assert_eq!(cells.of_node[node] as usize, cell);
            }
        }
    }

    #[test]
    #[should_panic(expected = "the directory was built over another graph")]
    fn a_directory_of_another_graph_is_caught() {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 0, 1], Vec::new());
        let _ = Customization::new(StaticGraph::new(edges), directory);
    }
}
