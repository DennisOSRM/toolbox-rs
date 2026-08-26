//! The cells of a partition read off a file as a search asks for them.
//!
//! # The same search, the other way round
//!
//! [`Customization`](crate::customization::Customization) works a cell out the
//! first time it is wanted and keeps it. This reads one off a disk the first
//! time it is wanted and keeps it until the room runs out. Both are
//! [`Overlay`], so the same search runs over either and cannot tell which it
//! has: what differs is where a table comes from and how long it stays.
//!
//! # Why a table is counted and not lent
//!
//! A customization lends a table out, nothing being able to move underneath
//! it. Here the room is bounded, so a table a search is reading is a table the
//! cache would otherwise be free to throw away to make room for the next one.
//! So a table is counted rather than lent: what comes back holds the table
//! open, and the cache cannot drop it until the search has done with it.
//!
//! That also means two tables may be held at once without the cache being
//! borrowed twice, which a search that walks two cells in one step needs.
//!
//! # What is held in bytes
//!
//! The cache is bounded by what the tables come to unpacked, not by how many
//! there are. A table of the finest level is a few hundred bytes and one of
//! the coarsest is most of a megabyte, so counting tables would mean a budget
//! that meant nothing: the same number of them is two megabytes or six hundred
//! depending on which ones a search happened to want.

use rayon::prelude::*;
use std::sync::{Arc, Mutex};

/// How many blocks are kept in hand while their cells are unpacked.
///
/// A block is wanted only while the cells around it are being read; a few is
/// enough to keep a search walking cells side by side from reading the same
/// block twice, and more would be room better given to the tables.
const BLOCKS_IN_HAND: usize = 8;

use crate::{
    block_map::BlockEntry,
    block_store::{BlockStore, NotRead},
    border_levels::{BorderLevels, Borders},
    cell_block::CellBlock,
    cell_tree::{CellFacts, CellTree},
    graph::{Arcs, NodeID},
    level_directory::CellId,
    lru::LRU,
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
    static_graph::StaticGraph,
};

/// One cell's table, as it is once read off the file.
#[derive(Debug)]
pub struct HeldTable {
    /// the table, row by row
    matrix: Vec<u32>,
    /// the transpose, for a search running backwards
    transposed: Vec<u32>,
    /// which node each place of the table is
    nodes: Vec<u32>,
}

impl HeldTable {
    /// What it takes up, which is what the budget counts.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + (self.matrix.capacity() + self.transposed.capacity() + self.nodes.capacity()) * 4
    }
}

impl CellTable for Arc<HeldTable> {
    #[inline]
    fn border_nodes(&self) -> &[u32] {
        &self.nodes
    }

    #[inline]
    fn row(&self, source: usize) -> &[u32] {
        let wide = self.nodes.len();
        &self.matrix[source * wide..(source + 1) * wide]
    }

    #[inline]
    fn column(&self, target: usize) -> &[u32] {
        let wide = self.nodes.len();
        &self.transposed[target * wide..(target + 1) * wide]
    }

    #[inline]
    fn place_of(&self, node: NodeID) -> Option<usize> {
        // the border nodes of a cell come out in increasing order, so this is
        // a search of a sorted run rather than a map to keep beside it
        let node = u32::try_from(node).ok()?;
        self.nodes.binary_search(&node).ok()
    }
}

/// What a store has been asked for and what it cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// tables the cache already had
    pub hits: u64,
    /// tables that had to be read off the file
    pub misses: u64,
    /// tables thrown away to make room
    pub evicted: u64,
    /// blocks read off the file, which is what a fault really costs
    pub reads: u64,
    /// tables unpacked out of a block already in hand
    pub unpacked: u64,
    /// what is held now, in bytes
    pub held: usize,
}

/// How much room a store may use, and how much of it is never given back.
///
/// # Why some levels are held and not cached
///
/// The coarse levels are read by every query that goes any distance and there
/// are few of them: on europe.ptv the top level is 18 MiB unpacked and the
/// finest is 186. Left to a cache they would be read, thrown away and read
/// again as the fine levels churned through the same room, and they are
/// exactly the tables a long query cannot do without.
///
/// So the coarse levels are held outright, from the top down, for as many as
/// fit the share asked for, and what is left of the budget is the cache. Held
/// tables need no lock and cannot be evicted; the rest take their chances.
///
/// Whatever the pinned share does not use goes to the cache rather than being
/// left idle: a share is a ceiling on what may be held, not a reservation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Budget {
    /// what the instance may come to in all, in bytes: the graph, the
    /// partition, the border levels, the store's own tables of contents, the
    /// arrays a search wants, and the cell tables
    pub bytes: usize,
    /// the most of what is left after the footing that may be held outright,
    /// as a share of that remainder
    pub pinned_share: f64,
}

/// What an instance costs before it holds a single cell table.
///
/// # Why this is not a detail
///
/// The budget is for the whole of an instance and the cell tables are the
/// smallest part of it. On a continent the graph is four hundred mebibytes and
/// the partition another three hundred, against tables that can be run in a
/// tenth of that: a budget set without counting them is not a budget for
/// anything a device has to hold.
///
/// So the footing is paid first and the tables get the remainder, and where
/// there is no remainder the budget is refused rather than quietly exceeded.
///
/// # What is fixed and what is not
///
/// The graph, the partition and the border levels go with the instance and
/// cannot be traded for anything; the store's map and cell tree go with how it
/// was packed. The searches are the one part that goes with how many run at
/// once, which is why they are counted per search rather than once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Footing {
    /// what the graph costs standing still: all of its arcs where it holds
    /// them, and only what finds a block where it pages them
    pub graph: u64,
    /// the bits the levels of the partition ask for, a node
    pub partition: u64,
    /// one byte an arc where they are kept apart, and nothing where the arcs
    /// carry their own
    pub border_levels: u64,
    /// one entry a block, and one a cell
    pub block_map: u64,
    pub cell_tree: u64,
    /// what the arrays of every search that may run at once come to
    pub searches: u64,
}

/// What a partition would take at a whole word a node, for a caller that has
/// not got one to ask.
fn nodes_words(nodes: usize) -> u64 {
    nodes as u64 * 16
}

impl Footing {
    /// What an instance over this graph and this store costs before tables.
    ///
    /// `searches` is how many searches may be running at once, since each
    /// keeps a table over the nodes of the graph for as long as it lives.
    #[must_use]
    pub fn of<G: Arcs<u32>>(graph: &G, tree: &CellTree, blocks: usize, searches: usize) -> Self {
        Self::with_partition(
            graph,
            nodes_words(graph.number_of_nodes()),
            tree,
            blocks,
            searches,
        )
    }

    /// The same, told what the partition actually takes.
    ///
    /// A partition is stored in the bits its levels ask for, which is a good
    /// deal less than a word a node, and only the partition itself knows how
    /// many that came to.
    #[must_use]
    pub fn with_partition<G: Arcs<u32>>(
        graph: &G,
        partition_bytes: u64,
        tree: &CellTree,
        blocks: usize,
        searches: usize,
    ) -> Self {
        let nodes = graph.number_of_nodes() as u64;
        Self {
            // what the graph says it costs standing still, which is all of its
            // arcs where it holds them and only an index where it pages them
            graph: graph.standing() as u64,
            partition: partition_bytes,
            // Nothing, where the arcs carry their own: the level rides in the
            // block with the arc and is read when the arc is. A caller whose
            // graph does not carry them says so with `with_borders`.
            border_levels: 0,
            block_map: blocks as u64 * size_of::<BlockEntry>() as u64,
            cell_tree: (0..tree.levels())
                .map(|level| tree.cells_on_level(level) as u64 * size_of::<CellFacts>() as u64)
                .sum(),
            // four bytes a node for the table a queue reads in one look; what
            // the heap itself takes goes with the widest run and not with the
            // graph, and is not standing room
            searches: searches as u64 * nodes * 4,
        }
    }

    /// The same, for an instance whose searches keep only what a run touched.
    ///
    /// A queue over an array costs four bytes a node standing still, which on
    /// a continent is sixty eight mebibytes before anybody searches. A queue
    /// over a map costs a hash on every look and nothing standing, and that is
    /// what an instance with a budget for the whole of it runs -- see
    /// [`SparseMldQuery`](crate::mld_query::SparseMldQuery).
    #[must_use]
    pub fn with_sparse_searches(mut self) -> Self {
        self.searches = 0;
        self
    }

    /// The same, for an instance keeping a byte an arc beside its graph.
    #[must_use]
    pub fn with_borders(mut self, arcs: usize) -> Self {
        self.border_levels = arcs as u64;
        self
    }

    /// What the whole of it comes to.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.graph
            + self.partition
            + self.border_levels
            + self.block_map
            + self.cell_tree
            + self.searches
    }
}

/// A budget that does not cover what the instance costs before tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooSmall {
    /// what the footing comes to
    pub footing: u64,
    /// what the budget was
    pub budget: usize,
}

impl std::fmt::Display for TooSmall {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a budget of {} bytes does not cover the {} the instance costs before any cell table",
            self.budget, self.footing
        )
    }
}

impl std::error::Error for TooSmall {}

impl Budget {
    /// A budget with half of it available to hold levels outright.
    #[must_use]
    pub fn of(bytes: usize) -> Self {
        Self {
            bytes,
            pinned_share: 0.5,
        }
    }

    /// What is left for the cell tables once the instance is paid for.
    ///
    /// # Errors
    ///
    /// Returns [`TooSmall`] where the budget does not cover the footing, which
    /// is a budget that cannot be met rather than one that will be tight.
    pub fn for_tables(&self, footing: &Footing) -> Result<usize, TooSmall> {
        let wants = usize::try_from(footing.total()).unwrap_or(usize::MAX);
        self.bytes.checked_sub(wants).ok_or(TooSmall {
            footing: footing.total(),
            budget: self.bytes,
        })
    }

    /// The lowest level worth holding outright, and the level count when none
    /// is: levels from here up are held, levels below are cached.
    ///
    /// The share is of what is left after the footing, not of the whole
    /// budget: half of a budget the graph has already spent is not room.
    #[must_use]
    pub fn pin_from(&self, tree: &CellTree, footing: &Footing) -> usize {
        let left = self.for_tables(footing).unwrap_or(0);
        let room = (left as f64 * self.pinned_share.clamp(0.0, 1.0)) as u64;
        let mut taken = 0_u64;
        let mut from = tree.levels();
        for level in (0..tree.levels()).rev() {
            let wants = tree.unpacked_bytes(level);
            if taken + wants > room {
                break;
            }
            taken += wants;
            from = level;
        }
        from
    }

    /// What the held levels come to, and what is left for the cache.
    ///
    /// Both are out of what the tables were left, so the two and the footing
    /// come to the budget.
    #[must_use]
    pub fn split(&self, tree: &CellTree, footing: &Footing) -> (u64, usize) {
        let left = self.for_tables(footing).unwrap_or(0);
        let from = self.pin_from(tree, footing);
        let pinned = (from..tree.levels())
            .map(|level| tree.unpacked_bytes(level))
            .sum::<u64>();
        (
            pinned,
            left.saturating_sub(usize::try_from(pinned).unwrap_or(usize::MAX)),
        )
    }
}

/// The cells of a partition, read off a file and kept while there is room.
pub struct PagedOverlay<G = StaticGraph<u32>, B = BorderLevels> {
    store: BlockStore,
    graph: G,
    partition: PackedPartition,
    borders: B,
    kept: Mutex<Kept>,
    budget: usize,
    /// the lowest level held outright, and the level count when none is
    pin_from: usize,
    /// the held levels, from `pin_from` up, and nothing for a cell with no table
    pinned: Vec<Vec<Option<Arc<HeldTable>>>>,
}

struct Kept {
    tables: LRU<(u8, CellId), Arc<HeldTable>>,
    bytes: usize,
    /// The blocks a table was last unpacked out of.
    ///
    /// A read is of a block and a table is of a cell, and a block holds many
    /// cells. Without this, two cells of one block are two reads of that block
    /// and two passes of the codec over it, which for a search walking cells
    /// side by side is nearly every read it makes.
    ///
    /// Held apart from the tables and given a fixed few, since a block is
    /// wanted only while the cells around it are being unpacked and a table is
    /// wanted for as long as the search keeps coming back to it.
    blocks: LRU<(u8, CellId), Arc<CellBlock>>,
    faults: Faults,
}

impl<G: Arcs<u32> + Sync, B: Borders + Sync> PagedOverlay<G, B> {
    /// Opens a store to read cells from, holding `budget` bytes of them.
    #[must_use]
    pub fn new(
        store: BlockStore,
        graph: G,
        partition: PackedPartition,
        borders: B,
        budget: usize,
    ) -> Self {
        Self {
            store,
            graph,
            partition,
            borders,
            // room for the entries, which the budget in bytes bounds; the
            // count is only what the map is made large enough for
            pin_from: usize::MAX,
            pinned: Vec::new(),
            kept: Mutex::new(Kept {
                tables: LRU::new_with_capacity(1 << 16),
                bytes: 0,
                blocks: LRU::new_with_capacity(BLOCKS_IN_HAND),
                faults: Faults::default(),
            }),
            budget,
        }
    }

    /// Opens a store and holds as many of the coarse levels as the budget
    /// allows, leaving the rest of it to the cache.
    ///
    /// The held levels are read here rather than on the first query, which is
    /// what a startup pays so that a first query does not.
    ///
    /// # Panics
    ///
    /// Panics if a level that was to be held cannot be read: a store missing
    /// what it says it holds is broken rather than sparse.
    #[must_use]
    pub fn within(
        store: BlockStore,
        graph: G,
        partition: PackedPartition,
        borders: B,
        budget: Budget,
    ) -> Self {
        let footing = Footing::with_partition(
            &graph,
            partition.bytes() as u64,
            store.tree(),
            store.map().len(),
            1,
        );
        let pin_from = budget.pin_from(store.tree(), &footing);
        let (_, cache) = budget.split(store.tree(), &footing);
        let levels = store.tree().levels();

        let mut held = Self::new(store, graph, partition, borders, cache);
        held.pinned = (pin_from..levels)
            .map(|level| {
                (0..held.store.tree().cells_on_level(level) as CellId)
                    .into_par_iter()
                    .map(|cell| match held.read(level, cell) {
                        Ok(table) => Some(table),
                        // a cell with no border has no table
                        Err(NotRead::NotHere) => None,
                        Err(why) => panic!("a level that was to be held: {why}"),
                    })
                    .collect()
            })
            .collect();
        held.pin_from = pin_from;
        held
    }

    /// The lowest level held outright, and the level count when none is.
    #[must_use]
    pub fn pinned_from(&self) -> usize {
        self.pin_from
    }

    /// What the held levels come to.
    #[must_use]
    pub fn pinned_bytes(&self) -> usize {
        self.pinned
            .iter()
            .flatten()
            .flatten()
            .map(|table| table.bytes())
            .sum()
    }

    /// What has been asked of it so far.
    ///
    /// # Panics
    ///
    /// Panics if another thread failed while holding the cache.
    #[must_use]
    pub fn faults(&self) -> Faults {
        let kept = self.kept.lock().expect("the cache is poisoned");
        Faults {
            held: kept.bytes,
            ..kept.faults
        }
    }

    /// Throws everything away, which a run that measures a cold store wants.
    ///
    /// # Panics
    ///
    /// Panics if another thread failed while holding the cache.
    pub fn forget(&self) {
        let mut kept = self.kept.lock().expect("the cache is poisoned");
        kept.tables.clear();
        kept.bytes = 0;
    }

    /// Reads a cell off the file, or hands back the one already held.
    ///
    /// # Errors
    ///
    /// What the store said, [`NotRead::NotHere`] being a region nobody
    /// downloaded rather than a fault.
    ///
    /// # Panics
    ///
    /// Panics if another thread failed while holding the cache.
    pub fn table_of(&self, level: usize, cell: CellId) -> Result<Arc<HeldTable>, NotRead> {
        // a held level needs no lock and cannot have been thrown away
        if level >= self.pin_from
            && let Some(held) = self.pinned.get(level - self.pin_from)
        {
            return held
                .get(cell as usize)
                .and_then(Clone::clone)
                .ok_or(NotRead::NotHere);
        }

        let key = (u8::try_from(level).map_err(|_| NotRead::NotHere)?, cell);
        {
            let mut kept = self.kept.lock().expect("the cache is poisoned");
            if let Some(held) = kept.tables.get(&key) {
                let held = held.clone();
                kept.faults.hits += 1;
                return Ok(held);
            }
        }

        let held = self.read(level, cell)?;

        let mut kept = self.kept.lock().expect("the cache is poisoned");
        kept.faults.misses += 1;
        kept.bytes += held.bytes();
        if let Some((_, was)) = kept.tables.push(&key, held.clone()) {
            kept.bytes -= was.bytes();
        }
        // and then down to the budget, oldest first
        while kept.bytes > self.budget && kept.tables.len() > 1 {
            let Some((_, was)) = kept.tables.pop_lru() else {
                break;
            };
            kept.bytes -= was.bytes();
            kept.faults.evicted += 1;
        }
        Ok(held)
    }

    /// Reads a cell off the file, holding nothing while it does.
    ///
    /// A fault is a disk read and a pass of a codec, and holding the cache
    /// through it would stop every other thread.
    fn read(&self, level: usize, cell: CellId) -> Result<Arc<HeldTable>, NotRead> {
        let entry = self.store.entry_of(level, cell).ok_or(NotRead::NotHere)?;
        let key = (
            u8::try_from(level).map_err(|_| NotRead::NotHere)?,
            entry.first_cell,
        );

        // the block this cell is in, off the file or out of the few in hand
        let block = {
            let mut kept = self.kept.lock().expect("the cache is poisoned");
            kept.blocks.get(&key).cloned()
        };
        let block = match block {
            Some(block) => {
                self.kept
                    .lock()
                    .expect("the cache is poisoned")
                    .faults
                    .unpacked += 1;
                block
            }
            None => {
                let read = Arc::new(self.store.block_at(&entry)?);
                let mut kept = self.kept.lock().expect("the cache is poisoned");
                kept.faults.reads += 1;
                kept.blocks.push(&key, read.clone());
                read
            }
        };

        let widths = self.store.widths_of(&entry, level);
        let which = (cell - entry.first_cell) as usize;
        let mut matrix = Vec::new();
        let mut nodes = Vec::new();
        block.unpack_into(which, &widths, &mut matrix);
        block.places_into(which, &widths, &mut nodes);
        let begins = self.store.tree().nodes_begin(level, cell);
        if nodes.is_empty() {
            nodes.extend((0..widths[which] as u32).map(|at| begins + at));
        } else {
            for node in &mut nodes {
                *node += begins;
            }
        }
        let wide = nodes.len();
        let mut transposed = vec![u32::MAX; matrix.len()];
        for source in 0..wide {
            for target in 0..wide {
                transposed[target * wide + source] = matrix[source * wide + target];
            }
        }
        Ok(Arc::new(HeldTable {
            matrix,
            transposed,
            nodes,
        }))
    }
}

impl<G: Arcs<u32> + Sync, B: Borders + Sync> Overlay for PagedOverlay<G, B> {
    type Graph = G;
    type Table<'a>
        = Arc<HeldTable>
    where
        Self: 'a;

    fn graph(&self) -> &Self::Graph {
        &self.graph
    }

    fn partition(&self) -> &PackedPartition {
        &self.partition
    }

    type Borders = B;

    fn borders(&self) -> &Self::Borders {
        &self.borders
    }

    fn levels(&self) -> usize {
        self.store.tree().levels()
    }

    fn cells_on_level(&self, level: usize) -> usize {
        self.store.tree().cells_on_level(level)
    }

    fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>> {
        // A cell nobody downloaded reads as a cell with no table, which is
        // what a cell with no border node reads as too. A search then does not
        // step over it, and answers with whatever it can reach otherwise.
        self.table_of(level, cell).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        block_codec::Codec,
        block_store::BlockWriter,
        cell_block::{CellBlock, CellEntry},
        cell_ordering::CellOrdering,
        cell_tree::CellTree,
        customization::Customization,
        edge::InputEdge,
        geometry::FPCoordinate,
        grid_graph::grid_directory,
        level_directory::LevelDirectory,
        mld_query::MldQuery,
        node_ordering::{NodeOrdering, Numbering},
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

    /// The whole pipeline: cells into key order, nodes into cell-path order,
    /// then the instance the store is built from.
    fn laid_out(side: usize) -> (Vec<InputEdge<u32>>, LevelDirectory) {
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
        (
            ordering.renumber(&edges),
            ordering.renumber_directory(&directory),
        )
    }

    /// Writes every cell of a customization into a store.
    fn pack_cells(
        customization: &Customization,
        tree: &CellTree,
        path: &std::path::Path,
        cells_a_block: usize,
    ) -> crate::block_map::BlockMap {
        let mut writer = BlockWriter::create(path).expect("a file to write");
        for level in 0..tree.levels() {
            let border_leads = level == 0;
            let mut at = 0;
            while at < tree.cells_on_level(level) {
                let upto = (at + cells_a_block).min(tree.cells_on_level(level));
                let mut matrices = Vec::new();
                let mut widths = Vec::new();
                let mut places = Vec::new();
                let mut holds = Vec::new();
                for cell in at..upto {
                    let cell = cell as CellId;
                    // The topmost cell holds the whole graph, so no arc leaves
                    // it and it has no border and no table. It goes into the
                    // block as a table of nothing rather than being left out,
                    // so that a block stays a run of cells.
                    let held = customization.distances_of(level, cell);
                    let wide = held.map_or(0, |table| table.border_nodes_of().len());
                    let mut matrix = Vec::with_capacity(wide * wide);
                    if let Some(table) = held {
                        for source in 0..wide {
                            matrix.extend_from_slice(table.row(source));
                        }
                    }
                    let begins = tree.nodes_begin(level, cell);
                    places.push(if border_leads {
                        Vec::new()
                    } else {
                        held.map_or_else(Vec::new, |table| {
                            table
                                .border_nodes_of()
                                .iter()
                                .map(|&node| node - begins)
                                .collect()
                        })
                    });
                    matrices.push(matrix);
                    widths.push(wide);
                    holds.push(tree.facts(level, cell).nodes as usize);
                }
                let entries = matrices
                    .iter()
                    .zip(&widths)
                    .zip(&places)
                    .zip(&holds)
                    .map(|(((matrix, &wide), places), &holds)| CellEntry {
                        matrix,
                        wide,
                        places,
                        holds,
                    })
                    .collect::<Vec<_>>();
                let block = CellBlock::of(level, at as CellId, &entries, border_leads);
                let keys = (
                    tree.range_of(level, at as CellId).0,
                    tree.range_of(level, (upto - 1) as CellId).1,
                );
                let nodes = (
                    tree.nodes_begin(level, at as CellId),
                    tree.nodes_begin(level, (upto - 1) as CellId)
                        + tree.facts(level, (upto - 1) as CellId).nodes
                        - tree.nodes_begin(level, at as CellId),
                );
                writer
                    .push(
                        &block,
                        keys,
                        (at as CellId, (upto - at) as u32),
                        nodes,
                        if level == 0 { Codec::Lz4 } else { Codec::Zstd },
                        3,
                    )
                    .expect("a block to write");
                at = upto;
            }
        }
        writer.finish().expect("a file to close")
    }

    /// The one that matters: the same search, unchanged, over the cells in
    /// memory and over the same cells read off a file, answering the same.
    #[test]
    fn a_search_over_a_file_answers_what_a_search_over_memory_does() {
        let side = 16;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let in_memory = Customization::new(StaticGraph::new(edges.clone()), directory.clone());

        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("blocks");
        let map = pack_cells(&in_memory, &tree, &path, 3);
        assert!(map.len() > 1, "the store is worth more than one block");

        let store = BlockStore::open(&path, map, tree).expect("a store to open");
        let paged = PagedOverlay::new(
            store,
            StaticGraph::new(edges.clone()),
            PackedPartition::of(&directory),
            BorderLevels::of(&graph, &partition),
            // deliberately small, so that tables are thrown away and read again
            8 * 1024,
        );

        let mut over_memory = MldQuery::new();
        let mut over_file = MldQuery::new();
        let mut pairs = 0;
        for source in (0..side * side).step_by(5) {
            for target in (0..side * side).step_by(7) {
                over_memory.clear();
                over_file.clear();
                let reached = over_memory.run(&in_memory, source, &[target]);
                assert_eq!(reached, over_file.run(&paged, source, &[target]));
                assert_eq!(
                    over_memory.distance(target),
                    over_file.distance(target),
                    "from {source} to {target}"
                );
                pairs += 1;
            }
        }

        let faults = paged.faults();
        assert!(pairs > 500, "the sweep is worth running");
        assert!(faults.misses > 0, "nothing was ever read off the file");
        assert!(faults.hits > 0, "nothing was ever found already held");
        assert!(faults.evicted > 0, "the budget was never reached");
    }

    /// The coarse levels are held and the fine ones are not, and the search
    /// answers the same either way.
    #[test]
    fn a_held_level_is_never_read_twice_and_answers_the_same() {
        let side = 16;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let in_memory = Customization::new(StaticGraph::new(edges.clone()), directory.clone());

        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("blocks");
        let map = pack_cells(&in_memory, &tree, &path, 3);

        // room enough for the two coarsest levels and no more, on top of what
        // the instance costs before any table: the budget is for the whole of
        // it and the graph is paid for first
        let levels = tree.levels();
        let wanted = tree.unpacked_bytes(levels - 1) + tree.unpacked_bytes(levels - 2);
        let footing = Footing::of(&graph, &tree, map.len(), 1);
        let budget = Budget {
            bytes: usize::try_from(footing.total() + wanted * 2).expect("a budget of that size"),
            pinned_share: 0.5,
        };
        let pin_from = budget.pin_from(&tree, &footing);
        assert_eq!(pin_from, levels - 2, "two levels were to be held");

        let store = BlockStore::open(&path, map, tree).expect("a store to open");
        let paged = PagedOverlay::within(
            store,
            StaticGraph::new(edges.clone()),
            PackedPartition::of(&directory),
            BorderLevels::of(&graph, &partition),
            budget,
        );
        assert_eq!(paged.pinned_from(), pin_from);
        assert!(paged.pinned_bytes() > 0, "nothing was held");

        // whatever the search does now, a held level is never read again
        let read_when_held = paged.faults().misses;
        let mut over_memory = MldQuery::new();
        let mut over_file = MldQuery::new();
        for source in (0..side * side).step_by(5) {
            for target in (0..side * side).step_by(7) {
                over_memory.clear();
                over_file.clear();
                over_memory.run(&in_memory, source, &[target]);
                over_file.run(&paged, source, &[target]);
                assert_eq!(
                    over_memory.distance(target),
                    over_file.distance(target),
                    "from {source} to {target}"
                );
            }
        }
        let faults = paged.faults();
        assert!(
            faults.misses > read_when_held,
            "the levels that were not held were never read"
        );
        // every read of a held level would have been a miss, and there are none
        for level in pin_from..levels {
            for cell in 0..paged.cells_on_level(level) as CellId {
                let before = paged.faults().misses;
                let _ = paged.distances_of(level, cell);
                assert_eq!(paged.faults().misses, before, "level {level} was read");
            }
        }
    }

    /// The whole of it on a file: the cell tables paged and the arcs paged
    /// under them, answering what an instance holding both in memory does.
    #[test]
    fn an_instance_with_its_arcs_on_a_file_too_answers_the_same() {
        use crate::paged_graph::{PagedGraph, pack};

        let side = 16;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let in_memory = Customization::new(StaticGraph::new(edges.clone()), directory.clone());

        let held = tempfile::tempdir().expect("a directory to write in");
        let tables = held.path().join("blocks");
        let map = pack_cells(&in_memory, &tree, &tables, 3);
        let arcs = held.path().join("arcs");
        let (arc_map, first_edges) = pack(
            &graph,
            &BorderLevels::of(&graph, &partition),
            Some(&tree),
            &arcs,
            32,
            Codec::Lz4,
            3,
        )
        .expect("a graph to pack");

        // both under budgets too small to hold what they are for, so both are
        // reading throughout rather than reading once and running in memory
        let paged_graph =
            PagedGraph::open(&arcs, arc_map, &first_edges, 8 * 1024).expect("a graph to open");
        let footing = Footing::of(&paged_graph, &tree, 4, 1);
        assert!(
            footing.graph < (crate::graph::Graph::number_of_edges(&graph) * 8) as u64 / 4,
            "a paged graph stood for most of itself: {} bytes",
            footing.graph
        );

        let store = BlockStore::open(&tables, map, tree.clone()).expect("a store to open");
        let budget = Budget {
            bytes: usize::try_from(footing.total()).expect("a size") + 16 * 1024,
            pinned_share: 0.5,
        };
        let paged = PagedOverlay::within(
            store,
            paged_graph,
            PackedPartition::of(&directory),
            BorderLevels::of(&graph, &partition),
            budget,
        );

        let mut over_memory = MldQuery::new();
        let mut over_file = MldQuery::new();
        let mut asked = 0;
        for source in (0..side * side).step_by(5) {
            for target in (0..side * side).step_by(7) {
                over_memory.clear();
                over_file.clear();
                over_memory.run(&in_memory, source, &[target]);
                over_file.run(&paged, source, &[target]);
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
            paged.graph().faults().reads > 0,
            "the arcs were never read off the file"
        );
    }

    /// The one that says what a budget is for: a graph counted, not assumed
    /// away.
    #[test]
    fn a_budget_pays_for_the_instance_before_it_pays_for_a_table() {
        let side = 16;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges);
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let footing = Footing::of(&graph, &tree, 4, 1);

        assert!(footing.graph > 0 && footing.partition > 0);
        assert_eq!(
            footing.total(),
            footing.graph
                + footing.partition
                + footing.border_levels
                + footing.block_map
                + footing.cell_tree
                + footing.searches,
            "the parts come to the whole"
        );

        // a budget under the footing is refused rather than quietly exceeded
        let short = Budget {
            bytes: usize::try_from(footing.total()).expect("a size") / 2,
            pinned_share: 0.5,
        };
        assert_eq!(
            short.for_tables(&footing),
            Err(TooSmall {
                footing: footing.total(),
                budget: short.bytes,
            })
        );
        // The coarsest level has no border and so costs nothing, and a level
        // that costs nothing fits any budget including one already spent. What
        // a budget it cannot meet holds is nothing that has a size.
        assert_eq!(
            short.split(&tree, &footing).0,
            0,
            "a budget it cannot meet held something with a cost"
        );
        assert_eq!(short.split(&tree, &footing).1, 0, "and left no cache");

        // and one over it leaves exactly the difference for the tables
        let over = Budget {
            bytes: usize::try_from(footing.total()).expect("a size") + 4096,
            pinned_share: 0.5,
        };
        assert_eq!(over.for_tables(&footing), Ok(4096));
        let (pinned, cache) = over.split(&tree, &footing);
        assert_eq!(
            usize::try_from(pinned).expect("a size") + cache,
            4096,
            "the held levels and the cache come to what was left, not to the budget"
        );
    }

    /// Two searches want two tables over the nodes, and a budget that counts
    /// one of them is a budget for an instance nobody is running.
    #[test]
    fn every_search_that_may_run_at_once_is_counted() {
        let side = 16;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges);
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

        let one = Footing::of(&graph, &tree, 4, 1);
        let four = Footing::of(&graph, &tree, 4, 4);
        assert_eq!(four.searches, one.searches * 4);
        assert_eq!(four.total() - one.total(), one.searches * 3);
    }

    #[test]
    fn a_budget_too_small_holds_only_what_costs_nothing() {
        let side = 8;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let budget = Budget {
            bytes: 8,
            pinned_share: 0.5,
        };
        // The topmost cell holds the whole graph, so no arc leaves it, so it
        // has no table and costs nothing to hold. A level that costs nothing
        // fits any budget, so what is asserted is that nothing which costs
        // anything was held.
        let from = budget.pin_from(&tree, &Footing::default());
        for level in from..tree.levels() {
            assert_eq!(
                tree.unpacked_bytes(level),
                0,
                "level {level} was held in eight bytes"
            );
        }
        assert_eq!(
            budget.split(&tree, &Footing::default()).0,
            0,
            "nothing with a cost was held"
        );
    }

    /// What the pinned share does not use is the cache's, not nobody's.
    #[test]
    fn room_the_held_levels_do_not_want_goes_to_the_cache() {
        let side = 8;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        let budget = Budget {
            bytes: 1 << 20,
            pinned_share: 0.5,
        };
        let (pinned, cache) = budget.split(&tree, &Footing::default());
        assert_eq!(
            cache,
            budget.bytes - usize::try_from(pinned).expect("a size"),
            "the cache gets everything the held levels did not"
        );
    }

    #[test]
    fn a_store_with_nothing_in_it_answers_that_it_has_nothing() {
        let side = 8;
        let (edges, directory) = laid_out(side);
        let graph = StaticGraph::new(edges.clone());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("blocks");
        let map = BlockWriter::create(&path)
            .expect("a file")
            .finish()
            .expect("a file to close");
        let store = BlockStore::open(&path, map, tree).expect("a store to open");
        let paged = PagedOverlay::new(
            store,
            StaticGraph::new(edges),
            PackedPartition::of(&directory),
            BorderLevels::of(&graph, &partition),
            1 << 20,
        );
        assert!(paged.distances_of(0, 0).is_none());
    }
}
