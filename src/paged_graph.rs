//! A graph read off a file a block at a time.
//!
//! # Why this and not the tables
//!
//! The cell tables paged first because they were what had just been built, and
//! they are the smaller half by a wide margin: on a continent the arcs come to
//! four hundred mebibytes and the tables run in a tenth of that. An instance
//! that pages only its tables still holds four hundred mebibytes of graph
//! whatever budget it was given, so a budget under about seven hundred and
//! fifty could not be met at all.
//!
//! # What is always held
//!
//! Two arrays, one entry a block: where its nodes begin, and where its arcs
//! do. They are what turns a node or an arc into a block to read, and they are
//! small enough to keep -- a continent packed into sixty four kibibyte blocks
//! has a few thousand of them, which is tens of kibibytes against the hundreds
//! of mebibytes they stand in for.
//!
//! Nothing else. A block is read, unpacked, held while it is wanted and let go
//! of when the room is.
//!
//! # Why an arc is asked for by number
//!
//! [`Arcs`] hands out arc numbers and takes them back, which is what a graph
//! held in memory wants and what a search was already written against. A block
//! store has to turn one back into a block, and it does that with the second
//! of the two arrays rather than by keeping a note of the last block it read:
//! a search asks about the arcs of a node one after another and would be
//! served by such a note, but it is not owed to it, and a store that answered
//! only the pattern it expected would be a store that answered wrongly the
//! first time anybody wrote a different search.

use std::{
    cell::RefCell,
    fs::File,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{
    block_codec::Codec,
    block_map::{BlockEntry, BlockMap},
    block_store::{BlockWriter, NotRead, read_at},
    border_levels::{BorderLevels, Borders},
    cell_tree::CellTree,
    graph::{Arcs, EdgeID, Graph, NodeID},
    graph_block::{GraphBlock, HeldArcs},
    pool::{Held, Key, Pool},
};

/// What a graph's blocks are filed under, since they are not cells.
const ARCS: u8 = u8::MAX;

/// Tells one open graph from another, so a thread's memo of the last block it
/// used is not offered to a graph it did not come from.
static NEXT_GRAPH: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// The last block this thread walked, and which graph it came out of.
    ///
    /// # Why a thread keeps one
    ///
    /// A search settles a node and walks its arcs, then settles another. The
    /// nodes were renumbered so that a cell's nodes are a run, and a search
    /// spends most of its time inside a cell, so the node it settles next is
    /// very often in the block it has just finished with. Going back through
    /// the lock and the list of what is held to be told so is most of what a
    /// paged graph costs over one in memory.
    ///
    /// This is only ever a shortcut. What it holds is what the store handed
    /// out, and where it holds nothing, or holds another graph's block, the
    /// question is asked of the store in the ordinary way. A block kept here
    /// after the store has let go of it is one block a thread, which is
    /// counted in the footing rather than left to be discovered.
    static RECENT: RefCell<Option<(usize, usize, Arc<HeldArcs>)>> =
        const { RefCell::new(None) };
}

/// Where every block's nodes and arcs begin.
///
/// One entry a block of each, in block order, which is node order. This is the
/// whole of what a paged graph holds without being asked.
#[derive(Clone, Debug, Default)]
pub struct GraphIndex {
    /// the first node of each block, sorted
    first_node: Vec<u32>,
    /// the first arc of each block, sorted, and one past the last arc of the
    /// last block on the end: a block's arcs are `first_edge[i]..first_edge[i
    /// + 1]`, and the sentinel is what makes that true of the last one too
    first_edge: Vec<u64>,
    nodes: usize,
    edges: usize,
}

impl GraphIndex {
    /// Reads the index off a map of graph blocks.
    ///
    /// # Panics
    ///
    /// Panics where the map holds a block that is not a run of arcs.
    #[must_use]
    pub fn of(map: &BlockMap, first_edges: &[u64]) -> Self {
        let entries = map.entries();
        assert_eq!(
            first_edges.len(),
            entries.len() + 1,
            "an arc number for every block and one past the end"
        );
        let first_node = entries.iter().map(|entry| entry.first_node).collect();
        let nodes = entries
            .last()
            .map_or(0, |entry| (entry.first_node + entry.nodes) as usize);
        // the sentinel is one past the last arc, which is how many there are
        let edges = first_edges.last().map_or(0, |&past| past as usize);
        Self {
            first_node,
            first_edge: first_edges.to_vec(),
            nodes,
            edges,
        }
    }

    /// Which block holds a node, and nothing where none does.
    #[must_use]
    pub fn block_of_node(&self, node: NodeID) -> Option<usize> {
        let node = u32::try_from(node).ok()?;
        let after = self.first_node.partition_point(|&first| first <= node);
        (after > 0 && (node as usize) < self.nodes).then(|| after - 1)
    }

    /// Which block holds an arc, and nothing where none does.
    #[must_use]
    pub fn block_of_edge(&self, edge: EdgeID) -> Option<usize> {
        let edge = edge as u64;
        let after = self.first_edge.partition_point(|&first| first <= edge);
        (after > 0 && (edge as usize) < self.edges).then(|| after - 1)
    }

    #[must_use]
    pub fn blocks(&self) -> usize {
        self.first_node.len()
    }

    /// Which arcs a block holds.
    #[must_use]
    pub fn edges_of_block(&self, which: usize) -> Option<(u64, u64)> {
        Some((
            *self.first_edge.get(which)?,
            *self.first_edge.get(which + 1)?,
        ))
    }

    /// What the index takes, which is what a paged graph costs standing still.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.first_node.capacity() * size_of::<u32>()
            + self.first_edge.capacity() * size_of::<u64>()
    }
}

/// A graph whose arcs are read off a file as they are wanted.
pub struct PagedGraph {
    arcs: File,
    map: BlockMap,
    index: GraphIndex,
    /// the one cache, shared with the tables and the ways
    pool: Arc<Pool>,
    /// which graph this is, for the thread's memo
    which: usize,
    /// how many blocks were read off the file
    reads: AtomicUsize,
}

impl PagedGraph {
    /// Opens a graph over a file of arc blocks.
    ///
    /// `budget` is what the blocks held may come to, unpacked.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong opening the file.
    pub fn open(
        arcs: &Path,
        map: BlockMap,
        first_edges: &[u64],
        pool: Arc<Pool>,
    ) -> std::io::Result<Self> {
        let index = GraphIndex::of(&map, first_edges);
        Ok(Self {
            arcs: File::open(arcs)?,
            map,
            index,
            pool,
            which: NEXT_GRAPH.fetch_add(1, Ordering::Relaxed),
            reads: AtomicUsize::new(0),
        })
    }

    #[must_use]
    pub fn index(&self) -> &GraphIndex {
        &self.index
    }

    /// How many blocks of arcs were read off the file.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    /// The cache it shares with everything else that reads.
    #[must_use]
    pub fn pool(&self) -> &Arc<Pool> {
        &self.pool
    }

    /// Reads a block off the file and unpacks it.
    fn read(&self, entry: &BlockEntry) -> Result<HeldArcs, NotRead> {
        let mut stored = vec![0_u8; entry.stored as usize];
        read_at(&self.arcs, entry.at, &mut stored)?;
        let codec = Codec::of(entry.codec).map_err(|_| NotRead::UnknownCodec(entry.codec))?;
        let bytes = codec
            .decode(&stored, entry.unpacked as usize)
            .map_err(NotRead::Corrupt)?;
        let block = rkyv::from_bytes::<GraphBlock, rkyv::rancor::Error>(&bytes)
            .map_err(|why| NotRead::Corrupt(why.to_string()))?;
        block
            .check_version()
            .map_err(|found| NotRead::Corrupt(format!("a block written under version {found}")))?;
        let mut held = HeldArcs::default();
        block.unpack_into(&mut held);
        Ok(held)
    }

    /// The block at an ordinal, read if the pool is not holding it.
    fn block(&self, which: usize) -> Result<Arc<HeldArcs>, NotRead> {
        let key = Key::Arcs(u32::try_from(which).map_err(|_| NotRead::NotHere)?);
        if let Some(Held::Arcs(held)) = self.pool.get(&key) {
            return Ok(held);
        }
        let entry = *self.map.entries().get(which).ok_or(NotRead::NotHere)?;
        let held = Arc::new(self.read(&entry)?);
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.pool.put(key, Held::Arcs(Arc::clone(&held)));
        Ok(held)
    }

    /// The block holding a node, read if it is not held.
    fn holding_node(&self, node: NodeID) -> Option<Arc<HeldArcs>> {
        let which = self.index.block_of_node(node)?;
        // the block this thread used last, where that is the one wanted
        if let Some(held) = RECENT.with_borrow(|recent| match recent {
            Some((graph, block, held)) if *graph == self.which && *block == which => {
                Some(Arc::clone(held))
            }
            _ => None,
        }) {
            return Some(held);
        }
        let held = self.block(which).ok()?;
        RECENT.with_borrow_mut(|recent| {
            *recent = Some((self.which, which, Arc::clone(&held)));
        });
        Some(held)
    }

    /// The block holding an arc, read if it is not held.
    fn holding_edge(&self, edge: EdgeID) -> Option<Arc<HeldArcs>> {
        self.block(self.index.block_of_edge(edge)?).ok()
    }
}

impl Arcs<u32> for PagedGraph {
    fn node_range(&self) -> std::ops::Range<NodeID> {
        0..self.index.nodes
    }

    fn edge_range(&self, node: NodeID) -> std::ops::Range<EdgeID> {
        self.holding_node(node).map_or(0..0, |held| {
            let (from, upto) = held.range_of(node);
            from as EdgeID..upto as EdgeID
        })
    }

    fn number_of_nodes(&self) -> usize {
        self.index.nodes
    }

    fn number_of_edges(&self) -> usize {
        self.index.edges
    }

    fn target(&self, edge: EdgeID) -> NodeID {
        self.holding_edge(edge)
            .and_then(|held| held.target(edge as u64))
            .expect("an arc the graph holds")
    }

    fn weight(&self, edge: EdgeID) -> u32 {
        self.holding_edge(edge)
            .and_then(|held| held.weight(edge as u64))
            .expect("an arc the graph holds")
    }

    /// Only the index stands: the blocks are a budget, not a footing.
    fn standing(&self) -> usize {
        self.index.bytes()
    }

    /// The block is found once and walked, rather than found again for every
    /// arc and again for every weight.
    fn for_each_arc(&self, node: NodeID, mut f: impl FnMut(NodeID, u32)) {
        let Some(held) = self.holding_node(node) else {
            return;
        };
        let (from, upto) = held.range_of(node);
        for edge in from..upto {
            let (target, weight) = (held.target(edge), held.weight(edge));
            if let (Some(target), Some(weight)) = (target, weight) {
                f(target, weight);
            }
        }
    }
}

/// Writes a graph's arcs out as blocks, through the store's own writer.
///
/// The blocks go through [`BlockWriter`], which is what the cell tables go
/// through: the same codec, the same entry, the same hash over the bytes, and
/// the same [`BlockMap`] out the other end. What differs is only what is in a
/// block and how it is keyed -- by the run of nodes it is for, since there is
/// no cell here to key it by.
pub struct ArcWriter {
    out: BlockWriter,
    first_edges: Vec<u64>,
    past_last_edge: u64,
    blocks: u32,
}

impl ArcWriter {
    /// Starts a file of arc blocks.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong making the file.
    pub fn create(path: &Path) -> std::io::Result<Self> {
        Ok(Self {
            out: BlockWriter::create(path)?,
            first_edges: Vec::new(),
            past_last_edge: 0,
            blocks: 0,
        })
    }

    /// Writes one block.
    ///
    /// `keys` is the span of cell keys the block's nodes fall in, taken off the
    /// same [`CellTree`](crate::cell_tree::CellTree) the tables are keyed by,
    /// so a run of arcs is looked up the way a run of tables is. Where a caller
    /// has no tree it may pass the node numbers, which is what the nodes were
    /// renumbered into anyway.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong serializing or writing it.
    pub fn push(
        &mut self,
        block: &GraphBlock,
        keys: (u128, u128),
        codec: Codec,
        effort: i32,
    ) -> std::io::Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(block)
            .map_err(|why| std::io::Error::other(format!("a block will not serialize: {why}")))?;
        self.out.push_bytes(
            &bytes,
            ARCS,
            keys,
            (self.blocks, 1),
            (
                u32::try_from(block.first_node()).expect("a node in four bytes"),
                u32::try_from(block.nodes()).expect("a run in four bytes"),
            ),
            codec,
            effort,
        )?;
        self.first_edges.push(block.first_edge());
        self.past_last_edge = block.first_edge() + block.edges() as u64;
        self.blocks += 1;
        Ok(())
    }

    /// Closes the file and hands back what says where everything is.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong flushing the file.
    pub fn finish(mut self) -> std::io::Result<(BlockMap, Vec<u64>)> {
        // one past the last arc, so that every block's arcs are the span
        // between two entries including the last block's
        self.first_edges.push(self.past_last_edge);
        let map = self.out.finish()?;
        Ok((map, self.first_edges))
    }
}

/// What the border levels are filed under, since they are neither cells nor
/// runs of arcs.
const BORDERS: u8 = u8::MAX - 1;

/// Writes the border levels out as blocks, one for each block of arcs.
///
/// A border level is a byte an arc saying the highest level at which the arc
/// leaves the cell of its source. It is settled when the store is packed and
/// does not change, and working it out means walking every arc of the graph --
/// which is the one thing a store that pages its arcs was built not to do. So
/// it is written down instead, through the same [`BlockWriter`] and the same
/// codec as everything else, cut on the same block boundaries as the arcs.
///
/// # Errors
///
/// Returns whatever went wrong writing the file.
pub fn pack_borders(
    levels: &[u8],
    first_edges: &[u64],
    path: &Path,
    codec: Codec,
    effort: i32,
) -> std::io::Result<BlockMap> {
    let mut writer = BlockWriter::create(path)?;
    for (block, span) in first_edges.windows(2).enumerate() {
        let (from, upto) = (span[0] as usize, span[1] as usize);
        writer.push_bytes(
            &levels[from..upto],
            BORDERS,
            (from as u128, upto.saturating_sub(1) as u128),
            (
                u32::try_from(block).expect("a block count in four bytes"),
                1,
            ),
            (
                u32::try_from(from).expect("an arc in four bytes"),
                u32::try_from(upto - from).expect("a run of arcs in four bytes"),
            ),
            codec,
            effort,
        )?;
    }
    writer.finish()
}

/// Reads back what [`pack_borders`] wrote, in one run a byte an arc.
///
/// # Errors
///
/// Returns whatever went wrong reading it.
pub fn read_borders(path: &Path, map: &BlockMap) -> Result<Vec<u8>, NotRead> {
    let file = File::open(path)?;
    let mut levels = Vec::new();
    for entry in map.entries() {
        let mut stored = vec![0_u8; entry.stored as usize];
        read_at(&file, entry.at, &mut stored)?;
        let codec = Codec::of(entry.codec).map_err(|_| NotRead::UnknownCodec(entry.codec))?;
        levels.extend_from_slice(
            &codec
                .decode(&stored, entry.unpacked as usize)
                .map_err(NotRead::Corrupt)?,
        );
    }
    Ok(levels)
}

/// Packs a graph into blocks of about so many arcs apiece.
///
/// `arcs_in_a_block` sets how much a read brings back: eight bytes an arc
/// unpacked, so eight thousand of them is a block of about sixty four
/// kibibytes once read, which is what the cell tables were found to want. A
/// node's arcs are never split across two blocks.
///
/// # Errors
///
/// Returns whatever went wrong writing the file.
pub fn pack<G: Graph<u32>>(
    graph: &G,
    borders: &BorderLevels,
    tree: Option<&CellTree>,
    path: &Path,
    arcs_in_a_block: usize,
    codec: Codec,
    effort: i32,
) -> std::io::Result<(BlockMap, Vec<u64>)> {
    // The keys a run of nodes falls under, off the same tree the cell tables
    // are keyed by. The nodes were renumbered so a cell's nodes are a run, so
    // a run of nodes is a run of keys and the two are looked up alike.
    let keys_of = |first: u32, upto: u32| -> (u128, u128) {
        let last = upto.saturating_sub(1);
        let plain = (u128::from(first), u128::from(last));
        let Some(tree) = tree else { return plain };
        match (
            tree.cell_holding_node(0, first as NodeID),
            tree.cell_holding_node(0, last as NodeID),
        ) {
            (Some(from), Some(to)) => (tree.range_of(0, from).0, tree.range_of(0, to).1),
            _ => plain,
        }
    };
    let mut writer = ArcWriter::create(path)?;
    let mut run: Vec<Vec<(u32, u32, u8)>> = Vec::new();
    let mut first_node = 0_u32;
    let mut first_edge = 0_u64;
    let mut in_run = 0_usize;

    for node in graph.node_range() {
        let out: Vec<(u32, u32, u8)> = graph
            .edge_range(node)
            .map(|edge| {
                (
                    u32::try_from(graph.target(edge)).expect("a node in four bytes"),
                    *graph.data(edge),
                    // the level the arc leaves its source's cell at rides with
                    // it, rather than standing in a byte an arc of its own
                    borders.highest_of(edge).map_or(0, |level| {
                        u8::try_from(level + 1).expect("a level in a byte")
                    }),
                )
            })
            .collect();
        // a node's arcs are kept together, so the run is closed before one that
        // would take it over rather than after
        if !run.is_empty() && in_run + out.len() > arcs_in_a_block {
            let block = GraphBlock::of(first_node, first_edge, &run);
            let upto = first_node + u32::try_from(run.len()).expect("a run in four bytes");
            writer.push(&block, keys_of(first_node, upto), codec, effort)?;
            first_node = upto;
            first_edge += in_run as u64;
            run.clear();
            in_run = 0;
        }
        in_run += out.len();
        run.push(out);
    }
    if !run.is_empty() {
        let block = GraphBlock::of(first_node, first_edge, &run);
        let upto = first_node + u32::try_from(run.len()).expect("a run in four bytes");
        writer.push(&block, keys_of(first_node, upto), codec, effort)?;
    }
    writer.finish()
}

/// The levels ride in the blocks with the arcs, so the graph is what answers
/// for them: nothing stands resident and a level is read when its arc is.
impl Borders for PagedGraph {
    #[inline]
    fn leaves_cell(&self, edge: EdgeID, level: usize) -> bool {
        self.holding_edge(edge)
            .and_then(|held| held.leaves_cell(edge as u64, level))
            .unwrap_or(false)
    }
}

/// A graph is often wanted twice over -- as the arcs and as what says which of
/// them leave a cell -- and one behind a count is the way to have it both ways
/// without holding it twice.
impl Arcs<u32> for Arc<PagedGraph> {
    fn node_range(&self) -> std::ops::Range<NodeID> {
        (**self).node_range()
    }
    fn edge_range(&self, node: NodeID) -> std::ops::Range<EdgeID> {
        (**self).edge_range(node)
    }
    fn number_of_nodes(&self) -> usize {
        (**self).number_of_nodes()
    }
    fn number_of_edges(&self) -> usize {
        (**self).number_of_edges()
    }
    fn target(&self, edge: EdgeID) -> NodeID {
        (**self).target(edge)
    }
    fn weight(&self, edge: EdgeID) -> u32 {
        (**self).weight(edge)
    }
    fn for_each_arc(&self, node: NodeID, f: impl FnMut(NodeID, u32)) {
        (**self).for_each_arc(node, f);
    }
    fn standing(&self) -> usize {
        (**self).standing()
    }
}

impl Borders for Arc<PagedGraph> {
    #[inline]
    fn leaves_cell(&self, edge: EdgeID, level: usize) -> bool {
        (**self).leaves_cell(edge, level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edge::InputEdge, static_graph::StaticGraph};

    fn grid(side: usize) -> StaticGraph<u32> {
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, 1 + (node % 7) as u32));
                    edges.push(InputEdge::new(node + 1, node, 1 + (node % 5) as u32));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, 2 + (node % 3) as u32));
                    edges.push(InputEdge::new(node + side, node, 2 + (node % 11) as u32));
                }
            }
        }
        StaticGraph::new(edges)
    }

    /// Border levels off a grid's own partition, which is what a real pack
    /// gets and what makes the arcs carry something worth reading back.
    fn borders_of(graph: &StaticGraph<u32>, side: usize) -> BorderLevels {
        use crate::{grid_graph::grid_directory, packed_partition::PackedPartition};
        let directory = grid_directory(side);
        BorderLevels::of(graph, &PackedPartition::of(&directory))
    }

    fn paged(
        graph: &StaticGraph<u32>,
        arcs: usize,
        budget: usize,
    ) -> (tempfile::TempDir, PagedGraph) {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("arcs");
        let (map, first_edges) = pack(
            graph,
            &BorderLevels::of_bytes(vec![0; Graph::number_of_edges(graph)]),
            None,
            &path,
            arcs,
            Codec::Lz4,
            3,
        )
        .expect("a graph to pack");
        let read =
            PagedGraph::open(&path, map, &first_edges, Pool::of(budget)).expect("a graph to open");
        (held, read)
    }

    /// The one that matters: every arc, off the file, is the arc the graph has.
    #[test]
    fn a_paged_graph_answers_what_the_graph_it_was_packed_from_does() {
        let graph = grid(24);
        // small enough that it is evicting throughout, which is the case worth
        // testing: a budget that holds everything never exercises a re-read
        let (_held, read) = paged(&graph, 64, 16 * 1024);

        assert_eq!(Arcs::number_of_nodes(&read), Graph::number_of_nodes(&graph));
        assert_eq!(Arcs::number_of_edges(&read), Graph::number_of_edges(&graph));

        for node in Graph::node_range(&graph) {
            let wanted = Graph::edge_range(&graph, node);
            assert_eq!(Arcs::edge_range(&read, node), wanted, "node {node}");
            for edge in wanted {
                assert_eq!(
                    Arcs::target(&read, edge),
                    Graph::target(&graph, edge),
                    "arc {edge} of node {node}"
                );
                assert_eq!(
                    Arcs::weight(&read, edge),
                    *Graph::data(&graph, edge),
                    "arc {edge} of node {node}"
                );
            }
        }
        let faults = read.pool().faults();
        assert!(faults.evicted > 0, "the budget was never binding");
        assert!(faults.held <= 16 * 1024, "the budget was exceeded");
    }

    #[test]
    fn a_budget_that_holds_everything_reads_each_block_once() {
        let graph = grid(16);
        let (_held, read) = paged(&graph, 128, 8 * 1024 * 1024);
        for _ in 0..3 {
            for node in Graph::node_range(&graph) {
                for edge in Arcs::edge_range(&read, node) {
                    let _ = Arcs::target(&read, edge);
                }
            }
        }
        assert_eq!(read.pool().faults().evicted, 0, "nothing was let go of");
        assert_eq!(
            read.reads(),
            read.index().blocks(),
            "a block was read more than once"
        );
    }

    #[test]
    fn a_node_no_block_holds_has_no_arcs() {
        let graph = grid(8);
        let (_held, read) = paged(&graph, 64, 1024 * 1024);
        let past = Graph::number_of_nodes(&graph);
        assert_eq!(Arcs::edge_range(&read, past), 0..0);
        assert_eq!(read.index().block_of_node(past), None);
        assert_eq!(
            read.index().block_of_edge(Graph::number_of_edges(&graph)),
            None
        );
    }

    #[test]
    fn a_node_keeps_its_arcs_in_one_block() {
        let graph = grid(16);
        let (_held, read) = paged(&graph, 4, 1024 * 1024);
        // asked for four arcs a block against nodes of up to four, so the runs
        // are cut by the nodes and never through one
        for node in Graph::node_range(&graph) {
            let range = Arcs::edge_range(&read, node);
            if range.is_empty() {
                continue;
            }
            let first = read.index().block_of_edge(range.start);
            let last = read.index().block_of_edge(range.end - 1);
            assert_eq!(first, last, "node {node} is split across blocks");
        }
    }

    /// The border levels are written down when the store is packed and read
    /// back, rather than worked out by walking every arc of a graph that is on
    /// a file.
    #[test]
    fn the_border_levels_read_back_as_the_walk_would_have_found_them() {
        use crate::{
            border_levels::BorderLevels, grid_graph::grid_directory,
            packed_partition::PackedPartition,
        };

        let side = 16;
        let graph = grid(side);
        let directory = grid_directory(side);
        let partition = PackedPartition::of(&directory);
        let walked = BorderLevels::of(&graph, &partition);

        let held = tempfile::tempdir().expect("a directory to write in");
        let arcs = held.path().join("arcs");
        let (_, first_edges) =
            pack(&graph, &walked, None, &arcs, 64, Codec::Lz4, 3).expect("a graph to pack");
        let borders = held.path().join("borders");
        let map = pack_borders(walked.as_bytes(), &first_edges, &borders, Codec::Lz4, 3)
            .expect("the levels to pack");
        assert!(map.len() > 1, "the pack is worth checking");

        let read = BorderLevels::of_bytes(read_borders(&borders, &map).expect("the levels"));
        assert_eq!(read.len(), walked.len(), "a different number of arcs");
        for edge in 0..walked.len() {
            assert_eq!(read.highest_of(edge), walked.highest_of(edge), "arc {edge}");
            for level in 0..directory.levels() {
                assert_eq!(
                    read.leaves_cell(edge, level),
                    walked.leaves_cell(edge, level),
                    "arc {edge} at level {level}"
                );
            }
        }
    }

    /// A plain Dijkstra, with no overlay under it at all, run over a graph
    /// that is on a file: the same search, the same answers.
    #[test]
    fn a_plain_dijkstra_over_a_paged_graph_answers_what_one_over_memory_does() {
        use crate::one_to_many_dijkstra::OneToManyDijkstra;

        let side = 20;
        let graph = grid(side);
        // a budget that cannot hold the graph, so the search is reading
        // throughout rather than reading once and running in memory
        let (_held, read) = paged(&graph, 128, 4 * 1024);

        let mut over_memory = OneToManyDijkstra::new();
        let mut over_file = OneToManyDijkstra::new();
        let nodes = Graph::number_of_nodes(&graph);
        let mut asked = 0;
        for source in (0..nodes).step_by(23) {
            let targets: Vec<NodeID> = (0..nodes).step_by(53).collect();
            over_memory.clear();
            over_file.clear();
            assert_eq!(
                over_memory.run(&graph, source, &targets),
                over_file.run(&read, source, &targets),
                "from {source}"
            );
            for &target in &targets {
                assert_eq!(
                    over_memory.distance(target),
                    over_file.distance(target),
                    "from {source} to {target}"
                );
                asked += 1;
            }
        }
        assert!(asked > 100, "the sweep is worth running");
        assert!(
            read.pool().faults().evicted > 0,
            "the budget was never binding"
        );
    }

    /// The blocks of arcs go into a store keyed the way the cell tables are, so
    /// a run of arcs is found by the keys of the cells its nodes are in.
    #[test]
    fn arcs_are_keyed_by_the_same_tree_the_tables_are() {
        use crate::{
            cell_tree::CellTree, geometry::FPCoordinate, grid_graph::grid_directory,
            packed_partition::PackedPartition,
        };

        let side = 16;
        let graph = grid(side);
        let directory = grid_directory(side);
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("arcs");
        let (map, first_edges) = pack(
            &graph,
            &borders_of(&graph, side),
            Some(&tree),
            &path,
            64,
            Codec::Lz4,
            3,
        )
        .expect("a graph to pack");

        // Every block carries the key span of the cells its nodes fall in.
        // They rise with the blocks only where the nodes have been renumbered
        // into cell order, which the pipeline does and this test does not, so
        // what is checked here is the keying itself.
        let entries = map.entries();
        assert!(entries.len() > 1, "the pack is worth checking");
        for entry in entries {
            let first = tree
                .cell_holding_node(0, entry.first_node as NodeID)
                .expect("a cell for the first node");
            assert_eq!(
                entry.first_key,
                tree.range_of(0, first).0,
                "a block is keyed by something other than its first node's cell"
            );
        }

        // and it still answers as the graph does
        let read =
            PagedGraph::open(&path, map, &first_edges, Pool::of(1 << 20)).expect("a graph to open");
        for node in Graph::node_range(&graph) {
            assert_eq!(
                Arcs::edge_range(&read, node),
                Graph::edge_range(&graph, node)
            );
        }
    }

    /// What the whole thing is for: the arcs on the file come to far less than
    /// the arcs in memory.
    #[test]
    fn the_arcs_on_the_file_are_smaller_than_the_arcs_in_memory() {
        let graph = grid(64);
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("arcs");
        let (map, _) = pack(
            &graph,
            &borders_of(&graph, 64),
            None,
            &path,
            8_192,
            Codec::Lz4,
            3,
        )
        .expect("a graph to pack");
        let (stored, _) = map.bytes();
        let in_memory =
            (Graph::number_of_edges(&graph) * 8 + Graph::number_of_nodes(&graph) * 4) as u64;
        assert!(
            stored * 2 < in_memory,
            "on the file {stored}, in memory {in_memory}"
        );
    }
}
