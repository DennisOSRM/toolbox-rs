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
    border_levels::BorderLevels,
    edge::InputEdge,
    graph::{Graph, NodeID},
    level_directory::{CellId, LevelDirectory},
    one_to_many_dijkstra::OneToManyDijkstra,
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
    static_graph::StaticGraph,
};
use log::debug;
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The distances between the border nodes of one cell, in the order the border
/// nodes are listed in.
pub struct CellDistances {
    /// The nodes on the edge of the cell, as four byte numbers.
    ///
    /// Read side by side with a row of the table above, once per arc a query
    /// takes across the cell, so the two are streamed together and both are
    /// worth keeping narrow. A graph of four thousand million nodes is not one
    /// this crate can hold anyway, as the tables address it with four bytes.
    pub border_nodes: Vec<u32>,
    /// What it costs to cross the cell, as four byte numbers.
    ///
    /// A query reads one of these for every arc it takes across a cell, and
    /// that walk is where such a query spends most of its time. Eight byte
    /// numbers would be twice the memory to stream for the same answers: on a
    /// partition of a continent the tables come to some sixty million entries,
    /// and the row of a coarse cell is a few hundred of them read one after
    /// another. Four bytes reach four thousand million, and the longest way
    /// across europe by the clock is under a million.
    matrix: Vec<u32>,
    /// The same table with its rows and columns swapped.
    ///
    /// A search running backwards through a cell wants what it costs to reach
    /// one border node from each of the others, which is a column of the table
    /// above: on the coarsest cells of a continent that is a few hundred reads
    /// with a couple of thousand bytes between one and the next, where the
    /// forward side reads the same count off one run of memory. Measured, the
    /// stride cost more at the top of the rank axis than running from both
    /// ends saved there. Held twice, both sides read a run.
    transposed: Vec<u32>,
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
    /// The nodes on the border of the cell, in the order the table is in.
    #[must_use]
    pub fn border_nodes_of(&self) -> &[u32] {
        &self.border_nodes
    }

    /// What the table takes up, near enough to count with.
    ///
    /// The three runs of numbers are exact. The map of places is not: it is
    /// asked how much room it took rather than how much it uses, and a hash
    /// map keeps room for more than it holds, so what is counted for it is
    /// that room at a key, a value and a byte of tag apiece.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.border_nodes.capacity() * size_of::<u32>()
            + self.matrix.capacity() * size_of::<u32>()
            + self.transposed.capacity() * size_of::<u32>()
            + self.place_of.capacity() * (size_of::<NodeID>() + size_of::<usize>() + 1)
    }

    /// Holds a table and the same table with rows and columns swapped.
    ///
    /// `matrix` is by row: entry `source * width + target` is what it costs to
    /// get from the border node in place `source` to the one in place
    /// `target`.
    fn holding(
        border_nodes: Vec<u32>,
        matrix: Vec<u32>,
        place_of: FxHashMap<NodeID, usize>,
    ) -> Self {
        let width = border_nodes.len();
        let mut transposed = vec![u32::MAX; matrix.len()];
        for source in 0..width {
            for target in 0..width {
                transposed[target * width + source] = matrix[source * width + target];
            }
        }
        Self {
            border_nodes,
            matrix,
            transposed,
            place_of,
        }
    }

    /// What it costs to get from one border node of the cell to another,
    /// both given as their place in `border_nodes`.
    #[must_use]
    pub fn distance(&self, source: usize, target: usize) -> usize {
        let across = self.matrix[source * self.border_nodes.len() + target];
        if across == u32::MAX {
            usize::MAX
        } else {
            across as usize
        }
    }

    /// What it costs to get from one border node to each of the others, as a
    /// row of the table.
    ///
    /// A search walks the whole row against `border_nodes`, and asking for the
    /// entries one at a time makes it work out where each sits: the width of
    /// the cell is read, multiplied, added and then checked against the length
    /// of the table, for every one of a couple of million arcs. The row is one
    /// piece of memory and the two are walked in step, so it is handed over
    /// whole and walked as it lies.
    ///
    /// An entry of `u32::MAX` is a pair with no way between them.
    #[must_use]
    pub fn row(&self, source: usize) -> &[u32] {
        let width = self.border_nodes.len();
        &self.matrix[source * width..(source + 1) * width]
    }

    /// What it costs to reach one border node from each of the others, as a
    /// column of the table.
    ///
    /// This is what a search running backwards through the cell reads, where a
    /// search running forwards reads a [`row`](Self::row). It is held as a run
    /// of memory of its own, so that both sides walk their entries in step
    /// with `border_nodes` rather than one of them striding the whole table.
    ///
    /// An entry of `u32::MAX` is a pair with no way between them.
    pub fn column(&self, target: usize) -> &[u32] {
        let width = self.border_nodes.len();
        &self.transposed[target * width..(target + 1) * width]
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
    graph: &StaticGraph<u32>,
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
            queue.push(Reverse((cost + *graph.data(edge) as usize, target)));
        }
    }
    settled
}

/// The cells of one level: which one each node sits in, which nodes each of
/// them holds, and which of those nodes sit on a border.
pub struct Level {
    pub of_node: Vec<CellId>,
    /// The nodes of every cell laid end to end, and where each cell starts in
    /// them.
    ///
    /// A vector per cell was a vector per cell: the finest level of a
    /// continent has half a million of them holding some thirty numbers each,
    /// and asking for half a million small pieces of room was the greater part
    /// of what working the level out cost. One run of numbers and one of
    /// offsets is the same answer, read the same way, in two allocations.
    starts: Vec<u32>,
    nodes: Vec<NodeID>,
    /// The highest level at which each node sits on a border, plus one, shared
    /// with every other level of the partition.
    border: Arc<Vec<u8>>,
    /// which level this is, to read `border` against
    level: usize,
    /// the cells of the level below that each cell of this one is built out of,
    /// and empty on the finest level, which is built from the graph itself
    pub built_from: Vec<Vec<CellId>>,
}

impl Level {
    /// How many cells the level holds.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.starts.len() - 1
    }

    /// The nodes of a cell, in increasing order.
    #[must_use]
    pub fn nodes_of(&self, cell: CellId) -> &[NodeID] {
        let from = self.starts[cell as usize] as usize;
        let to = self.starts[cell as usize + 1] as usize;
        &self.nodes[from..to]
    }

    /// What the level takes up, apart from the tables of its cells.
    ///
    /// The table of borders is left out, being one table for the whole
    /// partition rather than one a level, and counted by whoever asks about
    /// the partition instead.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.of_node.capacity() * size_of::<CellId>()
            + self.starts.capacity() * size_of::<u32>()
            + self.nodes.capacity() * size_of::<NodeID>()
            + self.built_from.capacity() * size_of::<Vec<CellId>>()
            + self
                .built_from
                .iter()
                .map(|children| children.capacity() * size_of::<CellId>())
                .sum::<usize>()
    }

    /// Whether a node sits on the border of its cell, an arc leaving it or
    /// reaching it from outside. Both count: a road network is directed, and a
    /// node that can only be entered from another cell is a way in that a path
    /// through the cell above may take.
    #[must_use]
    pub fn on_border(&self, node: NodeID) -> bool {
        self.border[node] as usize > self.level
    }
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
    /// how many arcs it ran over, a clique of the cells below among them
    pub arcs: usize,
    pub elapsed: Duration,
    /// What went on building the graph the searches then ran over.
    ///
    /// Split from the searches because the two answer to different things. The
    /// searches are the work the algorithm asks for and shrink only by asking
    /// for less of it; the building is a cost of how this happens to be
    /// arranged, and a level that spends its time there is a level with
    /// something to take away rather than something to make cleverer.
    pub building: Duration,
    /// and what went on the searches themselves
    pub searching: Duration,
    /// how many cells have been worked out so far, this one included
    pub customized_cells: usize,
    /// what all of them together have cost
    pub total: Duration,
}

/// How wide a cell may be and still be worth a Floyd-Warshall.
///
/// Two things decide whether a cell is better tabulated by working out every
/// pair at once than by a search from each of its border nodes, and the
/// measurements on a continent say both matter.
///
/// The finest level is searched over the arcs of the road network, where a
/// node has two or three of them. A search there touches almost nothing, and
/// working out every pair does the same work whatever the arcs are, so it
/// loses however narrow the cell is. Every level above is searched over the
/// cliques of the cells below, where the arcs go as the square of the nodes
/// and a search touches all of them. That is the case this is for.
///
/// And it is cubic in the nodes of the cell, so it wants a narrow one. Level
/// by level on Europe, one thread, against the searches it replaces:
///
/// ```text
///   level 0     36 nodes    4.15s against  3.74s
///   level 1     30 nodes    0.77s against  2.07s
///   level 2     45 nodes    0.49s against  1.69s
///   level 3    197 nodes    2.20s against  2.65s
///   level 4    505 nodes    3.88s against  3.07s
///   level 5   1169 nodes   11.69s against  3.40s
/// ```
///
/// The turn is between two hundred nodes and five hundred. The gains on either
/// side of it are slight and the losses past it are not, so the line is drawn
/// nearer the near side.
const WIDEST_FOR_FLOYD_WARSHALL: usize = 300;

/// The levels to work out every pair on, when `TOOLBOX_FLOYD_WARSHALL` says.
///
/// For measuring one way against the other, and nothing else: with nothing
/// asked for, each cell is judged on its own by the rule above.
fn floyd_warshall_levels() -> Option<&'static [bool]> {
    static LEVELS: OnceLock<Option<Vec<bool>>> = OnceLock::new();
    LEVELS
        .get_or_init(|| {
            let asked = std::env::var("TOOLBOX_FLOYD_WARSHALL").ok()?;
            let mut wanted = vec![false; 32];
            for level in asked
                .split(',')
                .filter_map(|word| word.trim().parse::<usize>().ok())
            {
                if let Some(slot) = wanted.get_mut(level) {
                    *slot = true;
                }
            }
            Some(wanted)
        })
        .as_deref()
}

/// Every distance within a cell, by Floyd-Warshall, of which the leading
/// `wide` rows and columns are the answer.
///
/// The table is the whole cell rather than its border, which is the trade: it
/// works out distances between nodes nobody asked about, and in exchange the
/// inner loop is a row read against a row written with nothing in it but a
/// compare and a move. No queue, no addressing, nothing to settle, and a
/// compiler is free to do several at once.
///
/// Row `k` is copied out before the sweep that uses it so that the two rows in
/// hand are not the same one, which lets the inner loop be a plain walk of two
/// slices rather than an indexed one the bounds checks stay in.
fn floyd_warshall(graph: &StaticGraph<u32>, nodes: usize, wide: usize) -> Vec<u32> {
    let mut table = vec![u32::MAX; nodes * nodes];
    for node in 0..nodes {
        table[node * nodes + node] = 0;
    }
    for source in 0..nodes {
        for edge in graph.edge_range(source) {
            let target = graph.target(edge);
            let weight = *graph.data(edge);
            let held = &mut table[source * nodes + target];
            *held = (*held).min(weight);
        }
    }

    let mut through = vec![u32::MAX; nodes];
    for step in 0..nodes {
        through.copy_from_slice(&table[step * nodes..(step + 1) * nodes]);
        for source in 0..nodes {
            let reach = table[source * nodes + step];
            // nothing goes through a node this one does not reach
            if reach == u32::MAX {
                continue;
            }
            let row = &mut table[source * nodes..(source + 1) * nodes];
            for (held, &onward) in row.iter_mut().zip(through.iter()) {
                // the unreachable is the largest there is, and saturating
                // keeps it there rather than wrapping it round to nothing
                let offered = reach.saturating_add(onward);
                *held = (*held).min(offered);
            }
        }
    }

    // the border nodes lead the numbering, so the answer is the corner of it
    let mut matrix = vec![u32::MAX; wide * wide];
    for source in 0..wide {
        matrix[source * wide..(source + 1) * wide]
            .copy_from_slice(&table[source * nodes..source * nodes + wide]);
    }
    matrix
}

/// What building and searching one cell needs, kept to be used for the next.
///
/// A cell is a small thing and there are six hundred thousand of them, so what
/// it costs to ask for the room is a real part of what a cell costs. Every one
/// of them was making a map, a list of arcs, an adjacency array and a search,
/// using them for a few microseconds and giving them all back. Under one
/// thread that is the allocator's fast path several million times; under eight
/// it is the allocator's slow path, and the customization of a continent spent
/// thirteen seconds of system time on it.
///
/// Held rather than pooled by size: what a cell wants grows to what the widest
/// cell of a level wanted and stays there, which for the finest level is a few
/// hundred arcs and for the coarsest a few hundred thousand.
#[derive(Default)]
struct Scratch {
    dijkstra: OneToManyDijkstra,
    /// where a node of the graph sits in the numbering of the cell
    of_node: FxHashMap<NodeID, usize>,
    /// the arcs of the cell, sorted in place and read into the graph
    edges: Vec<InputEdge<u32>>,
    /// the nodes of the cell that sit on its border, in order
    border_nodes: Vec<NodeID>,
}

thread_local! {
    /// Room kept for the next cell rather than asked for by each one.
    ///
    /// A cell is a small graph and a search over it a small object, so what it
    /// costs to make one is a real part of what a cell costs: on a continent
    /// there are six hundred thousand cells and six hundred thousand searches
    /// made and thrown away, and the queue inside one holds several vectors of
    /// its own. Kept per thread, the customization of a level running a cell
    /// to a thread and the threads sharing nothing else.
    ///
    /// A pool rather than one, so that a cell asking for the cells below it
    /// while it holds this cannot find the place empty and the borrow already
    /// taken. Bottom up nothing nests and the pool is one deep.
    static SCRATCH: RefCell<Vec<Scratch>> = const { RefCell::new(Vec::new()) };
}

/// The cells of a partition, worked out level by level as they are asked for.
pub struct Customization {
    graph: StaticGraph<u32>,
    directory: LevelDirectory,
    /// The cells of a level, and the nodes of each of them, worked out the
    /// first time that level is asked about. Walking the directory per node
    /// per arc would otherwise be paid on every request.
    levels: Mutex<FxHashMap<usize, Arc<Level>>>,
    /// called once per cell as it is worked out, for a caller that wants to
    /// report on it. What a cell is worth saying about differs by caller, and
    /// the bounding box a map wants means nothing to a checker.
    report: Option<Reporter>,
    /// The table of every cell, by level and then by cell, worked out the
    /// first time that cell is asked about and kept afterwards. Doing it up
    /// front would mean walking every cell of the input before the first
    /// request can be answered.
    ///
    /// Cell ids are places on their level and run from zero without gaps, so
    /// this is an index rather than a hash. That is what it is for. A query
    /// reads a table out of here for every node it settles, and behind a map
    /// under a lock that read was a hash of a pair, a probe into a table of
    /// two thirds of a million entries scattered over the heap, and a pair of
    /// atomics to hand out a counted pointer. Behind an index it is a load and
    /// a branch.
    ///
    /// The tables are boxed, so an empty slot is a pointer and a word rather
    /// than a whole [`CellDistances`]. A continent has cells enough for that
    /// to be the difference between ten megabytes of slots and a hundred.
    tabulated: Vec<Vec<OnceLock<Box<CellDistances>>>>,
    /// The cells of every level a node sits in, one word apiece, worked out on
    /// the first request for them.
    ///
    /// This is what a query asks per settled node, and the reason it is here
    /// rather than built per run is that it costs a walk of the whole graph.
    partition: OnceLock<PackedPartition>,
    /// The highest level at which each node sits on a border, plus one, and
    /// zero for a node no arc ever leaves.
    ///
    /// One walk of the arcs answers it for every level at once. Which level
    /// two nodes part at is a question the packed partition answers from their
    /// two words, and they part at every level below the coarsest one they
    /// part at, so one number a node says the whole of it. It was a walk of
    /// all forty-two million arcs per level, six times over, filling six
    /// tables of eighteen million bytes.
    border_of_node: OnceLock<Arc<Vec<u8>>>,
    /// The level each arc of the graph leaves a cell at, worked out on the
    /// first request for it. This is what a query reads instead of asking the
    /// partition about the far end of every arc it looks at.
    border_levels: OnceLock<BorderLevels>,
    /// how many cells have been customized so far, and how long that took in
    /// total. The customization runs cell by cell as the cells are asked
    /// about, so the sum is what the whole of it would have cost up front.
    customized_cells: AtomicUsize,
    customization_nanos: AtomicU64,
}

impl Customization {
    #[must_use]
    pub fn new(graph: StaticGraph<u32>, directory: LevelDirectory) -> Self {
        assert_eq!(
            graph.number_of_nodes(),
            directory.number_of_nodes(),
            "the directory was built over another graph"
        );
        // A slot apiece, so that a table is found by index later. How many
        // cells a level holds is a walk of the level below it, which is why it
        // is asked here once rather than per request.
        let tabulated = (0..directory.levels())
            .map(|level| {
                (0..directory.cells_on_level(level))
                    .map(|_| OnceLock::new())
                    .collect()
            })
            .collect();
        Self {
            graph,
            directory,
            levels: Mutex::new(FxHashMap::default()),
            report: None,
            tabulated,
            partition: OnceLock::new(),
            border_of_node: OnceLock::new(),
            border_levels: OnceLock::new(),
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
    pub const fn graph(&self) -> &StaticGraph<u32> {
        &self.graph
    }

    /// which cell each node sits in on each level
    pub const fn directory(&self) -> &LevelDirectory {
        &self.directory
    }

    /// The same, packed one word to a node, which is what a query reads.
    ///
    /// Worked out on the first request and kept, as it is a walk of the whole
    /// graph and every run over this partition wants the same answer.
    pub fn partition(&self) -> &PackedPartition {
        self.partition
            .get_or_init(|| PackedPartition::of(&self.directory))
    }

    /// The level each arc of the graph leaves a cell at.
    ///
    /// Worked out on the first request and kept. A search running backwards
    /// walks the graph turned around, whose arcs are held in another order, so
    /// that side builds its own over the reversed graph.
    pub fn border_levels(&self) -> &BorderLevels {
        self.border_levels
            .get_or_init(|| BorderLevels::of(&self.graph, self.partition()))
    }

    /// How many cells a level holds.
    ///
    /// Read off the room made for their tables, which was counted when this
    /// was built. Asking the directory instead is a walk of the level below,
    /// and on the finest level that is every node of the graph.
    ///
    /// # Panics
    ///
    /// Panics for a level the partition does not have.
    #[must_use]
    pub fn cells_on_level(&self, level: usize) -> usize {
        self.tabulated[level].len()
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
    ///
    /// This asks for the customization to itself, rather than sharing it as
    /// everything else here does. Handing a table out is a borrow that lasts
    /// as long as the caller reads it, and dropping the tables underneath a
    /// reader is the one thing the slots cannot be asked to allow.
    pub fn forget(&mut self) {
        for level in &mut self.tabulated {
            for slot in level {
                slot.take();
            }
        }
    }

    /// The cells of a level, worked out on the first request for it and kept.
    /// The highest level at which each node sits on a border, plus one.
    ///
    /// Walked once, whichever level asks for it first, and read by all of
    /// them. Two nodes an arc joins part at some coarsest level and at every
    /// level below it, so the coarsest is the whole answer for that arc, and
    /// the largest over the arcs of a node is the whole answer for the node.
    fn border_of_node(&self) -> &Arc<Vec<u8>> {
        self.border_of_node.get_or_init(|| {
            let partition = self.partition();
            // An arc puts both of its ends on a border, so a node is written
            // by whichever thread holds the arc rather than by the one holding
            // the node, and the two collide. A byte a node taken to the
            // largest offered settles it without a lock: what comes out does
            // not depend on the order the offers arrive in, so relaxed is
            // enough.
            let highest: Vec<AtomicU8> = (0..self.directory.number_of_nodes())
                .map(|_| AtomicU8::new(0))
                .collect();
            self.graph.node_range().into_par_iter().for_each(|source| {
                let word = partition.word(source);
                for edge in self.graph.edge_range(source) {
                    let target = self.graph.target(edge);
                    // an arc that stays inside every cell it is in puts
                    // neither of its ends on a border
                    let Some(parting) =
                        partition.highest_different_level(word, partition.word(target))
                    else {
                        continue;
                    };
                    // plus one, so that nought means no arc ever left
                    let reached = u8::try_from(parting + 1).expect("more levels than a byte holds");
                    highest[source].fetch_max(reached, Ordering::Relaxed);
                    highest[target].fetch_max(reached, Ordering::Relaxed);
                }
            });
            Arc::new(highest.into_iter().map(AtomicU8::into_inner).collect())
        })
    }

    pub fn level(&self, level: usize) -> Arc<Level> {
        if let Some(cells) = self
            .levels
            .lock()
            .expect("the level cache is poisoned")
            .get(&level)
        {
            return cells.clone();
        }

        // Read off the packed partition rather than the directory. The
        // directory holds a cell per node and a parent per cell per level, so
        // it answers by walking up as many parent tables as the level is high,
        // which is six random reads a node on the coarsest level of six. The
        // partition holds the whole ancestry of a node in one word, where the
        // cell at a level is a shift and a mask.
        let partition = self.partition();
        let of_node = (0..self.directory.number_of_nodes())
            .into_par_iter()
            .map(|node| partition.cell_in(partition.word(node), level))
            .collect::<Vec<_>>();

        // The nodes of each cell, counted and then placed, which leaves them
        // in increasing order within a cell. That order is relied on: the
        // border nodes of a cell lead its numbering in the order they are met
        // here, and a table is addressed by that numbering.
        let count = self.directory.cells_on_level(level);
        let mut starts = vec![0u32; count + 1];
        for &cell in &of_node {
            starts[cell as usize + 1] += 1;
        }
        for cell in 0..count {
            starts[cell + 1] += starts[cell];
        }
        let mut filled = starts.clone();
        let mut nodes = vec![0 as NodeID; of_node.len()];
        for (node, &cell) in of_node.iter().enumerate() {
            nodes[filled[cell as usize] as usize] = node as NodeID;
            filled[cell as usize] += 1;
        }

        let built_from = if level == 0 {
            Vec::new()
        } else {
            let mut children = vec![Vec::new(); count];
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
            starts,
            nodes,
            border: self.border_of_node().clone(),
            level,
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

    /// Hands out the distances of a cell, tabulating them on the first
    /// request, and `None` for a cell with no border node to tabulate.
    ///
    /// The table is lent out rather than counted, so a caller that reads one
    /// per settled node pays a load for it and nothing else.
    #[inline]
    pub fn distances_of(&self, level: usize, cell: CellId) -> Option<&CellDistances> {
        let slot = self.tabulated.get(level)?.get(cell as usize)?;
        if let Some(distances) = slot.get() {
            return Some(distances.as_ref());
        }

        // As with the levels, the first table to land is the one that is kept.
        // Two callers asking for the same cell at once both work it out, and
        // the tally counts both, as both were really paid for. This is asked
        // for by hand rather than through `get_or_init` because a cell is
        // built out of the cells below it, so tabulating one asks for others
        // while this is running, and because a cell with no border has no
        // table to put in the slot at all.
        let distances = self.tabulate(level, cell)?;
        let _ = slot.set(Box::new(distances));
        slot.get().map(Box::as_ref)
    }

    /// Builds the graph of a cell and runs a search from each of its border
    /// nodes. A cell is a small part of the input, so this is quick enough to
    /// happen while a caller waits for it.
    fn tabulate(&self, level: usize, cell: CellId) -> Option<CellDistances> {
        let started = Instant::now();
        let cells = self.level(level);
        // taken before the cell is built rather than before it is searched,
        // the building being the part that asks for the room
        let mut scratch = SCRATCH.with_borrow_mut(Vec::pop).unwrap_or_default();
        let building = Instant::now();
        if cell as usize >= cells.cells() {
            return None;
        }
        let nodes = cells.nodes_of(cell);

        // the border nodes lead the numbering, so that they are the leading
        // rows and columns of the matrix
        scratch.border_nodes.clear();
        scratch
            .border_nodes
            .extend(nodes.iter().copied().filter(|&node| cells.on_border(node)));
        if scratch.border_nodes.is_empty() {
            debug!("cell {cell} of level {level} has no border nodes");
            SCRATCH.with_borrow_mut(|pool| pool.push(scratch));
            return None;
        }

        let (cell_graph, searched) = if level == 0 {
            (
                self.subgraph_of(&cells, cell, nodes, &mut scratch),
                nodes.len(),
            )
        } else {
            // A cell is built out of the cells below it: what a path does
            // inside one of them is already tabulated, and what it does between
            // them is an arc of the graph. Searching that instead of the nodes
            // of the cell is what keeps a coarse level affordable.
            self.overlay_of(level, cell, &cells, &mut scratch)
        };

        let building = building.elapsed();
        let arcs = cell_graph.number_of_edges();

        // whichever graph it is, the border nodes lead its numbering
        let searching = Instant::now();
        let wide = scratch.border_nodes.len();
        // a cell of the finest level is searched over road arcs, which is the
        // one case where working out every pair is not worth it
        let every_pair = level > 0 && searched <= WIDEST_FOR_FLOYD_WARSHALL;
        let matrix = if floyd_warshall_levels().map_or(every_pair, |asked| asked[level.min(31)]) {
            floyd_warshall(&cell_graph, searched.max(wide), wide)
        } else {
            let mut matrix = vec![u32::MAX; wide * wide];
            for source in 0..wide {
                scratch.dijkstra.run_to_leading(&cell_graph, source, wide);
                let row = &mut matrix[source * wide..(source + 1) * wide];
                for (target, across) in row.iter_mut().enumerate() {
                    // what cannot be reached keeps the largest four byte
                    // number, and a cell that really did cost that much would
                    // be a graph nobody has
                    *across = u32::try_from(scratch.dijkstra.distance(target)).unwrap_or(u32::MAX);
                }
            }
            matrix
        };
        let searching = searching.elapsed();

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
                border_nodes: wide,
                searched,
                arcs,
                elapsed,
                building,
                searching,
                customized_cells,
                total,
            });
        } else {
            debug!(
                "cell {cell} of level {level}: {} nodes, {} of them on the border, searched over {searched}",
                nodes.len(),
                wide
            );
        }

        let place_of = scratch
            .border_nodes
            .iter()
            .enumerate()
            .map(|(place, &node)| (node, place))
            .collect();
        let border_nodes = scratch
            .border_nodes
            .iter()
            .map(|&node| u32::try_from(node).expect("the graph is too large to hold"))
            .collect();
        SCRATCH.with_borrow_mut(|pool| pool.push(scratch));
        Some(CellDistances::holding(border_nodes, matrix, place_of))
    }

    /// The arcs of the graph that stay inside a cell, with its border nodes
    /// numbered first. This is what the finest level is built from, as there is
    /// no level below it to take distances from.
    fn subgraph_of(
        &self,
        cells: &Level,
        cell: CellId,
        nodes: &[NodeID],
        scratch: &mut Scratch,
    ) -> StaticGraph<u32> {
        // TODO: faster hashmap implementation using tabhash or fibonacci hash
        let Scratch {
            of_node,
            edges,
            border_nodes,
            ..
        } = scratch;
        of_node.clear();
        edges.clear();
        for &node in border_nodes.iter() {
            of_node.insert(node, of_node.len());
        }
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
        let nodes = of_node.len().max(border_nodes.len());
        edges.sort_unstable();
        StaticGraph::from_sorted_slice(nodes, edges)
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
        scratch: &mut Scratch,
    ) -> (StaticGraph<u32>, usize) {
        let below = self.level(level - 1);
        let Scratch { of_node, edges, .. } = scratch;
        of_node.clear();
        edges.clear();

        // the border nodes of this cell lead the numbering, the border nodes of
        // the cells below follow
        for &node in cells
            .nodes_of(cell)
            .iter()
            .filter(|&&node| cells.on_border(node))
        {
            of_node.insert(node, of_node.len());
        }

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
                    // it came out of a table of four byte numbers, and the one
                    // that would not fit is the one the guard above turned away
                    let weight = u32::try_from(weight).expect("a cell wider than four bytes");
                    let next = of_node.len();
                    let from = *of_node.entry(from as usize).or_insert(next);
                    let next = of_node.len();
                    let to = *of_node.entry(to as usize).or_insert(next);
                    edges.push(InputEdge::new(from, to, weight));
                }
            }
        }

        // the arcs that cross from one cell below into another one of this cell
        for &child in &cells.built_from[cell as usize] {
            for &node in below.nodes_of(child) {
                if !below.on_border(node) {
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
        edges.sort_unstable();
        (StaticGraph::from_sorted_slice(searched, edges), searched)
    }
}

impl CellTable for &CellDistances {
    #[inline]
    fn border_nodes(&self) -> &[u32] {
        CellDistances::border_nodes_of(self)
    }

    #[inline]
    fn row(&self, source: usize) -> &[u32] {
        CellDistances::row(self, source)
    }

    #[inline]
    fn column(&self, target: usize) -> &[u32] {
        CellDistances::column(self, target)
    }

    #[inline]
    fn place_of(&self, node: NodeID) -> Option<usize> {
        CellDistances::place_of(self, node)
    }
}

/// The overlay a server runs on: every table in memory, worked out on the
/// first request and kept.
///
/// Handing a table out is a load. Nothing here can be evicted while a search
/// reads it, so the table is lent rather than guarded and the lifetime is the
/// customization's own.
impl Overlay for Customization {
    type Graph = StaticGraph<u32>;
    type Table<'a> = &'a CellDistances;

    #[inline]
    fn graph(&self) -> &Self::Graph {
        Customization::graph(self)
    }

    #[inline]
    fn partition(&self) -> &PackedPartition {
        Customization::partition(self)
    }

    #[inline]
    fn border_levels(&self) -> &BorderLevels {
        Customization::border_levels(self)
    }

    #[inline]
    fn levels(&self) -> usize {
        self.directory().levels()
    }

    #[inline]
    fn cells_on_level(&self, level: usize) -> usize {
        Customization::cells_on_level(self, level)
    }

    #[inline]
    fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>> {
        Customization::distances_of(self, level, cell)
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
            let reached = distances_within_cell(&self.graph, &cells.of_node, cell, from as usize);
            for (target, &to) in built.border_nodes.iter().enumerate() {
                let expected = reached.get(&(to as usize)).copied().unwrap_or(usize::MAX);
                let built = built.distance(source, target);
                if built != expected {
                    check.mismatches.push(Mismatch {
                        cell,
                        from: from as usize,
                        to: to as usize,
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

    /// A column of the table says what a row of it says, read the other way
    /// round. A search running backwards through a cell reads columns, so the
    /// two have to agree or one direction of it is wrong.
    #[test]
    fn a_column_reads_what_the_rows_hold() {
        let (graph, directory) = crate::grid_graph::grid(8, true);
        let customization = Customization::new(graph, directory);
        let distances = customization
            .distances_of(0, 0)
            .expect("the first cell has a table");

        let width = distances.border_nodes.len();
        assert!(width > 1, "a cell of a grid has a border");
        for target in 0..width {
            let column = distances.column(target);
            assert_eq!(
                column.len(),
                width,
                "a column is as tall as the cell is wide"
            );
            for (source, &across) in column.iter().enumerate() {
                assert_eq!(across, distances.row(source)[target]);
            }
        }
    }
    use super::*;
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// A square grid of `side` by `side` nodes, cut into squares of two by two
    /// on the finest level and of four by four above it. The arcs of a row run
    /// one way round when `both_ways` is not asked for, which is what a road
    /// network does and what makes the distances of a cell asymmetric.
    /// Two cells of two nodes each, joined on the level above.
    fn two_cells() -> Customization {
        let edges = vec![
            InputEdge::new(0, 1, 3_u32),
            InputEdge::new(1, 0, 3_u32),
            InputEdge::new(1, 2, 7_u32),
            InputEdge::new(2, 1, 7_u32),
            InputEdge::new(2, 3, 5_u32),
            InputEdge::new(3, 2, 5_u32),
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
            assert_eq!(distances.place_of(node as usize), Some(place));
        }
        // and a node that is not on this border has no place on it
        let elsewhere = *customization
            .distances_of(0, 1)
            .expect("no cell")
            .border_nodes
            .first()
            .expect("a cell with no border nodes");
        assert_eq!(distances.place_of(elsewhere as usize), None);
        assert_eq!(
            distances.distance_between(elsewhere as usize, elsewhere as usize),
            None
        );
    }

    /// Asking by node and asking by place are the same question.
    #[test]
    fn the_distance_between_two_nodes_is_the_one_in_their_places() {
        let customization = grid(8);
        let distances = customization.distances_of(1, 0).expect("no cell");

        for &source in &distances.border_nodes {
            for &target in &distances.border_nodes {
                let (source, target) = (source as usize, target as usize);
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
        let edges = vec![InputEdge::new(0, 1, 1_u32)];
        let directory = LevelDirectory::new(vec![0, 1], vec![vec![0, 0]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);

        let cells = customization.level(0);
        assert!(cells.on_border(0), "the node the arc leaves");
        assert!(cells.on_border(1), "the node the arc reaches");
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
        assert!(std::ptr::eq(first, second));
    }

    #[test]
    fn what_was_forgotten_is_worked_out_again() {
        let mut customization = two_cells();
        let first = customization
            .distances_of(0, 0)
            .expect("no cell 0")
            .border_nodes
            .clone();
        customization.forget();
        let second = customization.distances_of(0, 0).expect("no cell 0");

        // the tally is what says it was worked out twice. Holding the two
        // tables against each other would not: the second is free to land on
        // the memory the first was let go of.
        assert_eq!(customization.customized_cells(), 2, "the cell was kept");
        assert_eq!(first, second.border_nodes);
    }

    /// A border node whose arcs all leave its cell has none inside it, so it
    /// turns up in no arc of the subgraph. The graph still has to hold it, or
    /// a search started there reads past the end of the node array.
    #[test]
    fn a_cell_whose_arcs_all_leave_it_is_still_tabulated() {
        // nodes 0 and 1 sit in one cell and are joined only through node 2,
        // which sits in another, so the first cell holds no arc at all
        let edges = vec![
            InputEdge::new(0, 2, 1_u32),
            InputEdge::new(2, 0, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 1, 1_u32),
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
        let edges = vec![InputEdge::new(0, 1, 1_u32), InputEdge::new(1, 0, 1_u32)];
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
            InputEdge::new(0, 2, 1_u32),
            InputEdge::new(2, 0, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 1, 1_u32),
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

        for cell in 0..cells.cells() as CellId {
            let Some(built_up) = customization.distances_of(1, cell) else {
                continue;
            };

            // the same cell, searched over its own nodes instead
            let nodes = &cells.nodes_of(cell);
            let border = nodes
                .iter()
                .copied()
                .filter(|&node| cells.on_border(node))
                .collect::<Vec<_>>();
            let mut scratch = Scratch {
                border_nodes: border.clone(),
                ..Scratch::default()
            };
            let graph = customization.subgraph_of(&cells, cell, nodes, &mut scratch);
            let indices = (0..border.len() as NodeID).collect::<Vec<_>>();
            let mut dijkstra = OneToManyDijkstra::new();

            assert_eq!(
                built_up.border_nodes,
                border.iter().map(|&node| node as u32).collect::<Vec<_>>(),
                "cell {cell}"
            );
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
        for cell in 0..cells.cells() as CellId {
            let Some(built_up) = customization.distances_of(1, cell) else {
                continue;
            };
            let nodes = &cells.nodes_of(cell);
            let border = nodes
                .iter()
                .copied()
                .filter(|&node| cells.on_border(node))
                .collect::<Vec<_>>();
            let mut scratch = Scratch {
                border_nodes: border.clone(),
                ..Scratch::default()
            };
            let graph = customization.subgraph_of(&cells, cell, nodes, &mut scratch);
            let indices = (0..border.len() as NodeID).collect::<Vec<_>>();
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
        assert_eq!(cells.cells(), 16, "squares of two by two");
        for cell in 0..cells.cells() {
            let nodes = cells.nodes_of(cell as CellId);
            assert_eq!(nodes.len(), 4);
            for &node in nodes {
                assert_eq!(cells.of_node[node] as usize, cell);
            }
        }
    }

    #[test]
    #[should_panic(expected = "the directory was built over another graph")]
    fn a_directory_of_another_graph_is_caught() {
        let edges = vec![InputEdge::new(0, 1, 1_u32), InputEdge::new(1, 0, 1_u32)];
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
        let mut customization = grid(8);
        let (border_nodes, mut matrix) = {
            let built = customization.distances_of(1, 0).expect("no cell to bend");
            assert!(
                built.border_nodes.len() > 1,
                "a 1x1 matrix holds only a zero"
            );
            (built.border_nodes.clone(), built.matrix.clone())
        };
        matrix[1] += 100;
        let place_of = border_nodes
            .iter()
            .enumerate()
            .map(|(place, &node)| (node as usize, place))
            .collect();
        customization.tabulated[1][0] = OnceLock::from(Box::new(CellDistances::holding(
            border_nodes.clone(),
            matrix,
            place_of,
        )));

        let check = customization.check(1, 0);
        assert_eq!(
            check.mismatches.len(),
            1,
            "the check passed on a table that was bent"
        );
        let wrong = check.mismatches[0];
        assert_eq!(wrong.built, wrong.expected + 100);
        assert_eq!(wrong.from, border_nodes[0] as usize);
        assert_eq!(wrong.to, border_nodes[1] as usize);
    }
}
