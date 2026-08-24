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

use std::sync::{Arc, Mutex};

use crate::{
    block_store::{BlockStore, NotRead},
    border_levels::BorderLevels,
    graph::NodeID,
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
    /// what is held now, in bytes
    pub held: usize,
}

/// The cells of a partition, read off a file and kept while there is room.
pub struct PagedOverlay {
    store: BlockStore,
    graph: StaticGraph<u32>,
    partition: PackedPartition,
    borders: BorderLevels,
    kept: Mutex<Kept>,
    budget: usize,
}

struct Kept {
    tables: LRU<(u8, CellId), Arc<HeldTable>>,
    bytes: usize,
    faults: Faults,
}

impl PagedOverlay {
    /// Opens a store to read cells from, holding `budget` bytes of them.
    #[must_use]
    pub fn new(
        store: BlockStore,
        graph: StaticGraph<u32>,
        partition: PackedPartition,
        borders: BorderLevels,
        budget: usize,
    ) -> Self {
        Self {
            store,
            graph,
            partition,
            borders,
            // room for the entries, which the budget in bytes bounds; the
            // count is only what the map is made large enough for
            kept: Mutex::new(Kept {
                tables: LRU::new_with_capacity(1 << 16),
                bytes: 0,
                faults: Faults::default(),
            }),
            budget,
        }
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
        let key = (u8::try_from(level).map_err(|_| NotRead::NotHere)?, cell);
        {
            let mut kept = self.kept.lock().expect("the cache is poisoned");
            if let Some(held) = kept.tables.get(&key) {
                let held = held.clone();
                kept.faults.hits += 1;
                return Ok(held);
            }
        }

        // read outside the lock: a fault is a disk read and a pass of a codec,
        // and holding the cache through it would stop every other thread
        let mut matrix = Vec::new();
        let mut nodes = Vec::new();
        self.store.cell_into(level, cell, &mut matrix, &mut nodes)?;
        let wide = nodes.len();
        let mut transposed = vec![u32::MAX; matrix.len()];
        for source in 0..wide {
            for target in 0..wide {
                transposed[target * wide + source] = matrix[source * wide + target];
            }
        }
        let held = Arc::new(HeldTable {
            matrix,
            transposed,
            nodes,
        });

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
}

impl Overlay for PagedOverlay {
    type Graph = StaticGraph<u32>;
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

    fn border_levels(&self) -> &BorderLevels {
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
    fn pack(
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
        let map = pack(&in_memory, &tree, &path, 3);
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
