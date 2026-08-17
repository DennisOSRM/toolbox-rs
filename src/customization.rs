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
    /// Where each border node sits in `border_nodes`.
    ///
    /// A query reads the matrix once per arc it takes across a cell, and the
    /// matrix is addressed by place rather than by node. Walking the list to
    /// find the place would make every one of those arcs cost the width of the
    /// cell, which on the coarsest level of a continent is thousands of border
    /// nodes. It is worked out once here, where the list is built.
    place_of: FxHashMap<NodeID, usize>,
}

impl CellDistances {
    /// What it costs to get from one border node of the cell to another,
    /// both given as their place in `border_nodes`.
    #[must_use]
    pub fn distance(&self, source: usize, target: usize) -> usize {
        self.matrix[source * self.border_nodes.len() + target]
    }

    /// Where a node sits in `border_nodes`, and `None` for a node that is not
    /// on the border of this cell at all.
    #[must_use]
    pub fn place_of(&self, node: NodeID) -> Option<usize> {
        self.place_of.get(&node).copied()
    }

    /// What it costs to get from one border node of the cell to another, both
    /// given as themselves, and `None` unless the pair are both on the border.
    #[must_use]
    pub fn distance_between(&self, source: NodeID, target: NodeID) -> Option<usize> {
        Some(self.distance(self.place_of(source)?, self.place_of(target)?))
    }
}

/// What it costs to reach every node of a cell from one of its nodes without
/// leaving the cell, worked out on the graph itself with a plain Dijkstra over
/// a standard library heap.
///
/// This is the slow answer that the one built out of the cells below has to
/// match, and it deliberately walks a heap the crate has no hand in. The two
/// searches the crate ships sit on one heap of its own, so a reference built
/// on either of them goes wrong exactly where the customization does and
/// agrees with it for the wrong reason. That is not a hypothetical: the first
/// run of this check reported a cell distance of 32 against a graph that
/// offered 25, and the fault was in `decrease_key` of that heap rather than
/// anywhere near the overlay.
///
/// The nodes reached are held in a map rather than an array over the graph, as
/// a cell is a small part of it and the walk never steps outside.
///
/// # Panics
///
/// Panics in a debug build if the node it starts from is not in the cell,
/// which would answer about a cell the caller did not ask about.
pub(crate) fn distances_within_cell(
    graph: &StaticGraph<usize>,
    of_node: &[CellId],
    cell: CellId,
    from: NodeID,
) -> FxHashMap<NodeID, usize> {
    use std::{cmp::Reverse, collections::BinaryHeap};

    debug_assert_eq!(of_node[from], cell, "the node is not in the cell");

    let mut settled = FxHashMap::default();
    let mut queue = BinaryHeap::new();
    queue.push(Reverse((0_usize, from)));
    while let Some(Reverse((cost, node))) = queue.pop() {
        if settled.contains_key(&node) {
            continue;
        }
        settled.insert(node, cost);
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            // a path of a cell may not step outside of it
            if of_node[target] != cell || settled.contains_key(&target) {
                continue;
            }
            queue.push(Reverse((cost + *graph.data(edge), target)));
        }
    }
    settled
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

/// What is called with each cell as it is worked out.
type Reporter = Box<dyn Fn(&CellReport) + Send + Sync>;

/// What a cell costs to work out, handed to whoever is watching.
pub struct CellReport<'a> {
    pub level: usize,
    pub cell: CellId,
    /// the nodes the cell holds
    pub nodes: &'a [NodeID],
    /// how many of them sit on its border, which is the side of its matrix
    pub border_nodes: usize,
    /// how many nodes the search ran over, which on a level above the finest
    /// is the border nodes of the cells below rather than all of their nodes
    pub searched: usize,
    pub elapsed: Duration,
    /// how many cells have been worked out so far, this one included
    pub customized_cells: usize,
    /// what all of them together have cost
    pub total: Duration,
}

/// The cells of a partition, worked out level by level as they are asked for.
pub struct Customization {
    graph: StaticGraph<usize>,
    directory: LevelDirectory,
    /// The cells of a level, and the nodes of each of them, worked out the
    /// first time that level is asked about. Walking the directory per node
    /// per arc would otherwise be paid on every request.
    levels: Mutex<FxHashMap<usize, Arc<Level>>>,
    /// called once per cell as it is worked out, for a caller that wants to
    /// report on it. What a cell is worth saying about differs by caller, and
    /// the bounding box a map wants means nothing to a checker.
    report: Option<Reporter>,
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
            report: None,
            tabulated: Mutex::new(FxHashMap::default()),
            customized_cells: AtomicUsize::new(0),
            customization_nanos: AtomicU64::new(0),
        }
    }

    /// Hands each cell to the given function as it is worked out.
    ///
    /// A cell with no border node is not reported, as there is nothing to work
    /// out for it and no table is made.
    #[must_use]
    pub fn watched_by(mut self, report: impl Fn(&CellReport) + Send + Sync + 'static) -> Self {
        self.report = Some(Box::new(report));
        self
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

        let (cell_graph, of_node, searched) = if level == 0 {
            (
                self.subgraph_of(&cells, cell, nodes, &border_nodes),
                None,
                nodes.len(),
            )
        } else {
            // A cell is built out of the cells below it: what a path does
            // inside one of them is already tabulated, and what it does between
            // them is an arc of the graph. Searching that instead of the nodes
            // of the cell is what keeps a coarse level affordable.
            let (graph, of_node, searched) = self.overlay_of(level, cell, &cells);
            (graph, Some(of_node), searched)
        };

        // whichever graph it is, the border nodes lead its numbering
        let border = (0..border_nodes.len()).collect::<Vec<_>>();
        let mut matrix = vec![usize::MAX; border_nodes.len() * border_nodes.len()];
        let mut dijkstra = OneToManyDijkstra::new();
        for &source in &border {
            dijkstra.run(&cell_graph, source, &border);
            for &target in &border {
                matrix[source * border_nodes.len() + target] = dijkstra.distance(target);
            }
        }
        drop(of_node);

        // the searches are what the customization of a cell costs, so the
        // clock is read once they are done
        let elapsed = started.elapsed();
        let customized_cells = self.customized_cells.fetch_add(1, Ordering::Relaxed) + 1;
        let total = Duration::from_nanos(
            self.customization_nanos
                .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed),
        ) + elapsed;

        if let Some(report) = &self.report {
            report(&CellReport {
                level,
                cell,
                nodes,
                border_nodes: border_nodes.len(),
                searched,
                elapsed,
                customized_cells,
                total,
            });
        } else {
            debug!(
                "cell {cell} of level {level}: {} nodes, {} of them on the border, searched over {searched}",
                nodes.len(),
                border_nodes.len()
            );
        }

        let place_of = border_nodes
            .iter()
            .enumerate()
            .map(|(place, &node)| (node, place))
            .collect();
        Some(CellDistances {
            border_nodes,
            matrix,
            place_of,
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

    /// The graph a cell of a level above the finest is searched over: one arc
    /// per pair of border nodes of a cell below, carrying what it costs to
    /// cross that cell, and the arcs of the graph that run between two of them.
    ///
    /// A path through the cell alternates between the two: it crosses a cell
    /// below from one of its border nodes to another, then takes an arc into
    /// the next one. The border nodes of this cell are border nodes of the
    /// cells below it too, so every search starts and ends on one.
    fn overlay_of(
        &self,
        level: usize,
        cell: CellId,
        cells: &Level,
    ) -> (StaticGraph<usize>, FxHashMap<NodeID, usize>, usize) {
        let below = self.level(level - 1);

        // the border nodes of this cell lead the numbering, the border nodes of
        // the cells below follow
        let mut of_node = FxHashMap::default();
        for &node in cells.nodes_of_cell[cell as usize]
            .iter()
            .filter(|&&node| cells.on_border[node])
        {
            of_node.insert(node, of_node.len());
        }

        let mut edges = Vec::new();
        for &child in &cells.built_from[cell as usize] {
            let Some(distances) = self.distances_of(level - 1, child) else {
                // a cell below with no border cannot be entered or left, so no
                // path of this cell runs through it
                continue;
            };
            for (source, &from) in distances.border_nodes.iter().enumerate() {
                for (target, &to) in distances.border_nodes.iter().enumerate() {
                    let weight = distances.distance(source, target);
                    if source == target || weight == usize::MAX {
                        continue;
                    }
                    let next = of_node.len();
                    let from = *of_node.entry(from).or_insert(next);
                    let next = of_node.len();
                    let to = *of_node.entry(to).or_insert(next);
                    edges.push(InputEdge::new(from, to, weight));
                }
            }
        }

        // the arcs that cross from one cell below into another one of this cell
        for &child in &cells.built_from[cell as usize] {
            for &node in &below.nodes_of_cell[child as usize] {
                if !below.on_border[node] {
                    continue;
                }
                for edge in self.graph.edge_range(node) {
                    let target = self.graph.target(edge);
                    if cells.of_node[target] != cell || below.of_node[target] == child {
                        continue;
                    }
                    let next = of_node.len();
                    let from = *of_node.entry(node).or_insert(next);
                    let next = of_node.len();
                    let to = *of_node.entry(target).or_insert(next);
                    edges.push(InputEdge::new(from, to, *self.graph.data(edge)));
                }
            }
        }

        // A cell whose parts have nothing running between them has an overlay
        // of no arcs, and the answer for it is a table saying each border node
        // reaches itself and nothing else. That is a table all the same, so it
        // is worked out rather than refused.
        //
        // The graph is asked for the nodes the overlay has, as a border node
        // that no arc of it touches would otherwise be missing from it and a
        // search started there would read past its end.
        let searched = of_node.len();
        let graph = StaticGraph::new_with_nodes(searched, edges);
        (graph, of_node, searched)
    }
}

/// A cell distance that is not what the graph says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub cell: CellId,
    pub from: NodeID,
    pub to: NodeID,
    /// what the customization worked out
    pub built: usize,
    /// what a search over the graph itself found, `usize::MAX` for a border
    /// node the other one cannot reach without leaving the cell
    pub expected: usize,
}

/// What checking one cell came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CellCheck {
    /// how many ordered pairs of border nodes were held against the graph
    pub pairs: u64,
    pub mismatches: Vec<Mismatch>,
    /// whether the cell has a border at all. One without cannot be entered or
    /// left, so there is nothing to tabulate and nothing to check.
    pub has_border: bool,
}

impl Customization {
    /// Holds every distance of a cell against a search over the graph itself.
    ///
    /// This is the slow way round and the point of it: the cell is worked out
    /// the way a query would have it, out of the cells below, and then again
    /// from each of its border nodes by a walk of the graph that knows nothing
    /// of levels. On the finest level the two are close relatives, so it says
    /// little there; above it they share nothing but the input.
    pub fn check(&self, level: usize, cell: CellId) -> CellCheck {
        let cells = self.level(level);
        let Some(built) = self.distances_of(level, cell) else {
            return CellCheck::default();
        };

        let mut check = CellCheck {
            has_border: true,
            ..Default::default()
        };
        for (source, &from) in built.border_nodes.iter().enumerate() {
            let reached = distances_within_cell(&self.graph, &cells.of_node, cell, from);
            for (target, &to) in built.border_nodes.iter().enumerate() {
                let expected = reached.get(&to).copied().unwrap_or(usize::MAX);
                let built = built.distance(source, target);
                if built != expected {
                    check.mismatches.push(Mismatch {
                        cell,
                        from,
                        to,
                        built,
                        expected,
                    });
                }
                check.pairs += 1;
            }
        }
        check
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng, prelude::StdRng};

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

    /// A square grid with a partition cut over it, as
    /// [`crate::grid_graph`] builds them.
    fn grid_with(side: usize, both_ways: bool) -> Customization {
        let (graph, directory) = crate::grid_graph::grid(side, both_ways);
        Customization::new(graph, directory)
    }

    fn grid(side: usize) -> Customization {
        grid_with(side, true)
    }

    /// A query reads the matrix by node and the matrix is addressed by place,
    /// so the two have to line up.
    #[test]
    fn a_border_node_knows_where_it_sits_in_the_matrix() {
        let customization = two_cells();
        let distances = customization.distances_of(0, 0).expect("no cell");

        for (place, &node) in distances.border_nodes.iter().enumerate() {
            assert_eq!(distances.place_of(node), Some(place));
        }
        // and a node that is not on this border has no place on it
        let elsewhere = *customization
            .distances_of(0, 1)
            .expect("no cell")
            .border_nodes
            .first()
            .expect("a cell with no border nodes");
        assert_eq!(distances.place_of(elsewhere), None);
        assert_eq!(distances.distance_between(elsewhere, elsewhere), None);
    }

    /// Asking by node and asking by place are the same question.
    #[test]
    fn the_distance_between_two_nodes_is_the_one_in_their_places() {
        let customization = grid(8);
        let distances = customization.distances_of(1, 0).expect("no cell");

        for &source in &distances.border_nodes {
            for &target in &distances.border_nodes {
                let places = distances.distance(
                    distances.place_of(source).expect("not on the border"),
                    distances.place_of(target).expect("not on the border"),
                );
                assert_eq!(distances.distance_between(source, target), Some(places));
            }
        }
    }

    #[test]
    fn every_cell_is_reported_to_whoever_watches() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let heard = seen.clone();
        let customization = two_cells().watched_by(move |report| {
            heard.lock().expect("the log is poisoned").push((
                report.level,
                report.cell,
                report.nodes.len(),
            ));
        });

        customization.distances_of(0, 0).expect("no cell 1");
        customization.distances_of(0, 1).expect("no cell 2");
        // the cell that was kept is not worked out again, so it is not
        // reported again either
        customization.distances_of(0, 0).expect("no cell 1");

        assert_eq!(
            *seen.lock().expect("the log is poisoned"),
            vec![(0, 0, 2), (0, 1, 2)]
        );
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

    /// A coarse cell whose parts have nothing running between them is still a
    /// cell with a table: every border node reaches itself and no other.
    #[test]
    fn a_cell_whose_parts_are_not_joined_is_still_tabulated() {
        // two cells of the finest level under one cell above, joined to the
        // world outside but not to each other
        let edges = vec![
            InputEdge::new(0, 2, 1_usize),
            InputEdge::new(2, 0, 1_usize),
            InputEdge::new(1, 2, 1_usize),
            InputEdge::new(2, 1, 1_usize),
        ];
        // nodes 0 and 1 in cells 0 and 1, which meet above; node 2 sits apart
        let directory = LevelDirectory::new(vec![0, 1, 2], vec![vec![0, 0, 1]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);

        let distances = customization
            .distances_of(1, 0)
            .expect("the cell has border nodes");
        assert_eq!(distances.border_nodes, vec![0, 1]);
        assert_eq!(distances.distance(0, 0), 0);
        assert_eq!(distances.distance(1, 1), 0);
        assert_eq!(distances.distance(0, 1), usize::MAX);
    }

    #[test]
    fn a_cell_built_from_the_cells_below_says_what_the_graph_says() {
        let customization = grid(8);
        let cells = customization.level(1);

        for cell in 0..cells.nodes_of_cell.len() as CellId {
            let Some(built_up) = customization.distances_of(1, cell) else {
                continue;
            };

            // the same cell, searched over its own nodes instead
            let nodes = &cells.nodes_of_cell[cell as usize];
            let border = nodes
                .iter()
                .copied()
                .filter(|&node| cells.on_border[node])
                .collect::<Vec<_>>();
            let graph = customization.subgraph_of(&cells, cell, nodes, &border);
            let indices = (0..border.len()).collect::<Vec<_>>();
            let mut dijkstra = OneToManyDijkstra::new();

            assert_eq!(built_up.border_nodes, border, "cell {cell}");
            for (source, _) in border.iter().enumerate() {
                dijkstra.run(&graph, source, &indices);
                for (target, _) in border.iter().enumerate() {
                    assert_eq!(
                        built_up.distance(source, target),
                        dijkstra.distance(target),
                        "cell {cell}, from {source} to {target}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_cell_of_one_way_streets_says_what_the_graph_says() {
        let customization = grid_with(8, false);
        let cells = customization.level(1);

        let mut asymmetric = 0;
        for cell in 0..cells.nodes_of_cell.len() as CellId {
            let Some(built_up) = customization.distances_of(1, cell) else {
                continue;
            };
            let nodes = &cells.nodes_of_cell[cell as usize];
            let border = nodes
                .iter()
                .copied()
                .filter(|&node| cells.on_border[node])
                .collect::<Vec<_>>();
            let graph = customization.subgraph_of(&cells, cell, nodes, &border);
            let indices = (0..border.len()).collect::<Vec<_>>();
            let mut dijkstra = OneToManyDijkstra::new();

            for source in 0..border.len() {
                dijkstra.run(&graph, source, &indices);
                for target in 0..border.len() {
                    assert_eq!(
                        built_up.distance(source, target),
                        dijkstra.distance(target),
                        "cell {cell}, from {source} to {target}"
                    );
                    if built_up.distance(source, target) != built_up.distance(target, source) {
                        asymmetric += 1;
                    }
                }
            }
        }
        assert!(
            asymmetric > 0,
            "the graph has to hold a pair whose distance differs by direction"
        );
    }

    /// A graph and a hierarchy over it, both drawn without a pattern. A grid
    /// cut into squares is regular enough to get right by accident, and this
    /// is what says otherwise.
    fn random(rng: &mut StdRng, nodes: usize, levels: usize) -> Customization {
        let mut edges = Vec::new();
        // a path through every node first, so that most of the graph hangs
        // together, then arcs that go wherever
        for node in 0..nodes - 1 {
            edges.push(InputEdge::new(node, node + 1, 1 + rng.random_range(0..9)));
            if rng.random_range(0..100) < 70 {
                edges.push(InputEdge::new(node + 1, node, 1 + rng.random_range(0..9)));
            }
        }
        for _ in 0..nodes {
            let source = rng.random_range(0..nodes);
            let target = rng.random_range(0..nodes);
            if source != target {
                edges.push(InputEdge::new(source, target, 1 + rng.random_range(0..9)));
            }
        }

        // cells that are cut out of the numbering rather than out of the graph
        let mut cells = 0;
        let base = (0..nodes)
            .map(|node| {
                if node == 0 || rng.random_range(0..100) < 25 {
                    cells += 1;
                }
                cells - 1
            })
            .collect::<Vec<_>>();
        let mut parents = Vec::new();
        let mut below = cells as usize;
        for _ in 1..levels {
            let mut above = 0;
            let table = (0..below)
                .map(|cell| {
                    if cell == 0 || rng.random_range(0..100) < 40 {
                        above += 1;
                    }
                    above - 1
                })
                .collect::<Vec<_>>();
            below = above as usize;
            parents.push(table);
        }

        Customization::new(StaticGraph::new(edges), LevelDirectory::new(base, parents))
    }

    /// Holds every cell of every level against the graph.
    fn check_against_the_graph(customization: &Customization, what: &str) {
        let mut checked = 0;
        for level in 0..customization.directory().levels() {
            for cell in 0..customization.directory().cells_on_level(level) as CellId {
                let check = customization.check(level, cell);
                assert!(
                    check.mismatches.is_empty(),
                    "{what}: level {level}, {:?}",
                    check.mismatches.first()
                );
                checked += check.pairs;
            }
        }
        assert!(checked > 0, "{what}: nothing was checked");
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

    #[test]
    fn every_cell_of_a_grid_says_what_the_graph_says() {
        check_against_the_graph(&grid(8), "a grid of two way streets");
        check_against_the_graph(&grid_with(8, false), "a grid of one way streets");
    }

    #[test]
    fn every_cell_of_a_graph_without_a_pattern_says_what_the_graph_says() {
        let mut rng = StdRng::seed_from_u64(0x_1234_5678);
        for round in 0..8 {
            let customization = random(&mut rng, 60 + round * 20, 3 + round % 3);
            check_against_the_graph(&customization, &format!("round {round}"));
        }
    }

    #[test]
    fn a_cell_that_disagrees_with_the_graph_is_reported() {
        // The check has to fail on tables that are wrong, or its passing says
        // nothing. The table of one cell is bent by hand and put back where
        // the check reads it from.
        //
        // It is this cell's own table that is bent rather than one of the
        // cells below it. Bending a cell below only makes the overlay route
        // around it, and on a grid there is always a way round.
        let customization = grid(8);
        let built = customization.distances_of(1, 0).expect("no cell to bend");
        assert!(
            built.border_nodes.len() > 1,
            "a 1x1 matrix holds only a zero"
        );

        let mut matrix = built.matrix.clone();
        matrix[1] += 100;
        customization
            .tabulated
            .lock()
            .expect("the tabulation cache is poisoned")
            .insert(
                (1, 0),
                Arc::new(CellDistances {
                    place_of: built
                        .border_nodes
                        .iter()
                        .enumerate()
                        .map(|(place, &node)| (node, place))
                        .collect(),
                    border_nodes: built.border_nodes.clone(),
                    matrix,
                }),
            );

        let check = customization.check(1, 0);
        assert_eq!(
            check.mismatches.len(),
            1,
            "the check passed on a table that was bent"
        );
        let wrong = check.mismatches[0];
        assert_eq!(wrong.built, wrong.expected + 100);
        assert_eq!(wrong.from, built.border_nodes[0]);
        assert_eq!(wrong.to, built.border_nodes[1]);
    }
}
