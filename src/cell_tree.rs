//! The assembled partition as a tree of cells, and the key of a cell in it.
//!
//! # Why this exists beside the directory
//!
//! [`LevelDirectory`] holds the assembly's answer in the form the assembly
//! left it: which cell each node is in, and which cell of the level above each
//! cell belongs to. That is enough to ask about a node and nothing else. It
//! cannot say what a cell holds without walking every node, how many of those
//! sit on its border without walking every arc, or where it is on the ground
//! at all.
//!
//! A store that hands out one cell at a time has to answer all three before it
//! reads anything, so they are worked out once and written down.
//!
//! # The key of a cell
//!
//! A cell is named by the path from the root down to it, packed into one word
//! the way [`PackedPartition`] already packs the path of a node: a level gets
//! as many bits as its cells need, the coarse levels take the high bits, and
//! the levels below the cell are left at nought.
//!
//! Two things follow, and they are the whole reason for the format.
//!
//! Sorting keys sorts by the coarsest cell first and by finer cells within it,
//! which is the order a walk of the tree would visit them in. And a cell's
//! subtree is exactly the keys from its own to its own with every bit below it
//! set — one range, with nothing outside the subtree falling inside it and
//! nothing inside falling outside. So a range of keys is a subtree, a block
//! holds a range, and a range nobody has is a part of the map nobody
//! downloaded.
//!
//! This is why the node numbering has to be [`Numbering::CellPath`]: the keys
//! come out in one range either way, but the nodes only do under that one.
//!
//! [`Numbering::CellPath`]: crate::node_ordering::Numbering::CellPath

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    bounding_box::BoundingBox,
    geometry::FPCoordinate,
    graph::Graph,
    level_directory::{CellId, LevelDirectory},
    packed_partition::PackedPartition,
};

/// The version this is written under. A reader that does not know a version
/// refuses the file rather than reading it as though it were another one.
pub const VERSION: u16 = 1;

/// Where a cell sits in the tree, as the path from the root packed into a word.
///
/// Ordering is by the word, which is by coarsest cell first. A key carries the
/// level it names so that [`last`](Self::last) knows how much of the word
/// below it is subtree rather than path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Archive, Serialize, Deserialize)]
pub struct CellKey {
    /// the path, with every level below `level` left at nought
    word: u128,
    /// the level the key names, counted from the finest
    level: u8,
}

impl CellKey {
    /// The word itself, which is the first key of the subtree.
    #[must_use]
    pub fn first(&self) -> u128 {
        self.word
    }

    #[must_use]
    pub fn level(&self) -> usize {
        self.level as usize
    }

    /// The last key of the subtree: the path with everything below it set.
    ///
    /// `below` is where this key's level begins in the word, which the tree
    /// knows and the key does not carry.
    #[must_use]
    pub fn last(&self, below: u32) -> u128 {
        self.word | ((1_u128 << below) - 1)
    }
}

/// What a cell holds, where it is, and how much of it faces outward.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CellFacts {
    /// how many nodes the cell holds
    pub nodes: u32,
    /// how many of them an arc leaves the cell from or reaches from outside
    pub on_border: u32,
}

/// The assembled partition, with the parts of it a store has to ask about.
#[derive(Clone, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CellTree {
    version: u16,
    /// where each level's bits begin in a key, finest first; the last entry is
    /// the width of a whole key in bits
    begins_at: Vec<u32>,
    /// per level above the finest, where the children of each cell begin in
    /// `children`, with one past the end in the last entry
    starts: Vec<Vec<u32>>,
    /// per level above the finest, the cells of the level below that each cell
    /// of this one is built out of, in increasing order
    children: Vec<Vec<CellId>>,
    /// per level below the topmost, the cell of the level above that each cell
    /// belongs to
    ///
    /// Held rather than looked for. The children of a cell are one run, but a
    /// child's number is not where it sits in that run, so the offsets cannot
    /// be asked which run a given child fell in.
    parents: Vec<Vec<CellId>>,
    /// per level, what each of its cells holds
    facts: Vec<Vec<CellFacts>>,
    /// per level, the box each of its cells lies in, and an invalid box for a
    /// cell holding no node with a coordinate
    bounds: Vec<Vec<BoundingBox>>,
}

impl CellTree {
    /// Works the tree out from the assembly's answer and the graph it was cut
    /// from.
    ///
    /// One walk of the nodes counts what each cell holds and grows its box,
    /// one walk of the arcs counts what faces outward, and one walk of the
    /// parents lays out the children. Nothing here searches.
    ///
    /// # Panics
    ///
    /// Panics if the graph, the partition and the coordinates are not over the
    /// same nodes.
    #[must_use]
    pub fn of<G: Graph<u32>>(
        directory: &LevelDirectory,
        partition: &PackedPartition,
        graph: &G,
        coordinates: &[FPCoordinate],
    ) -> Self {
        let levels = directory.levels();
        let nodes = directory.number_of_nodes();
        assert_eq!(graph.number_of_nodes(), nodes, "another graph");
        assert_eq!(coordinates.len(), nodes, "another set of coordinates");

        let counts = (0..levels)
            .map(|level| directory.cells_on_level(level))
            .collect::<Vec<_>>();

        // what each cell holds, and where it is
        let mut facts = counts
            .iter()
            .map(|&count| {
                vec![
                    CellFacts {
                        nodes: 0,
                        on_border: 0
                    };
                    count
                ]
            })
            .collect::<Vec<Vec<_>>>();
        let mut bounds = counts
            .iter()
            .map(|&count| vec![BoundingBox::invalid(); count])
            .collect::<Vec<Vec<_>>>();
        for (node, coordinate) in coordinates.iter().enumerate() {
            let word = partition.word(node);
            let at = BoundingBox::from_coordinate(coordinate);
            for (level, (holds, box_of)) in facts.iter_mut().zip(bounds.iter_mut()).enumerate() {
                let cell = partition.cell_in(word, level) as usize;
                holds[cell].nodes += 1;
                box_of[cell].extend_with(&at);
            }
        }

        // and how much of each faces outward. An arc that leaves a cell puts
        // both of its ends on that cell's border, and a node is counted once
        // however many arcs leave it, so the counting is over nodes with the
        // arcs asked about rather than over arcs.
        // An arc leaving a cell puts *both* of its ends on a border: a node an
        // arc only reaches from outside is a way into the cell, and a path
        // through the cell above may come in by it. So the arcs are walked
        // once and both ends written, rather than each node asked about its
        // own outgoing arcs, which would miss every node that can only be
        // entered. On europe.ptv that is 261,684 nodes of the finest level,
        // nearly a tenth of its border.
        let mut highest = vec![0_u8; nodes];
        for node in 0..nodes {
            let word = partition.word(node);
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                if let Some(parting) =
                    partition.highest_different_level(word, partition.word(target))
                {
                    // plus one, so that nought means no arc ever left
                    let reached =
                        u8::try_from(parting + 1).expect("more levels than a byte counts");
                    highest[node] = highest[node].max(reached);
                    highest[target] = highest[target].max(reached);
                }
            }
        }
        for (node, &reached) in highest.iter().enumerate() {
            // a node parts from a neighbour at some coarsest level and at
            // every level below it, so it is on a border at all of them
            let word = partition.word(node);
            for (level, holds) in facts
                .iter_mut()
                .enumerate()
                .take((reached as usize).min(levels))
            {
                let cell = partition.cell_in(word, level) as usize;
                holds[cell].on_border += 1;
            }
        }

        // the children of each cell, laid out in one run per level
        let mut starts = Vec::with_capacity(levels.saturating_sub(1));
        let mut children = Vec::with_capacity(levels.saturating_sub(1));
        let mut parents = Vec::with_capacity(levels.saturating_sub(1));
        for (level, &above) in counts.iter().enumerate().take(levels).skip(1) {
            let above_of = directory.parents_on_level(level - 1);
            let mut begins = vec![0_u32; above + 1];
            for &parent in above_of {
                begins[parent as usize + 1] += 1;
            }
            for cell in 0..above {
                begins[cell + 1] += begins[cell];
            }
            let mut filled = begins.clone();
            let mut held = vec![0 as CellId; above_of.len()];
            for (child, &parent) in above_of.iter().enumerate() {
                held[filled[parent as usize] as usize] =
                    CellId::try_from(child).expect("more cells than a cell id counts");
                filled[parent as usize] += 1;
            }
            starts.push(begins);
            children.push(held);
            parents.push(above_of.to_vec());
        }

        Self {
            version: VERSION,
            begins_at: partition.level_layout().to_vec(),
            starts,
            children,
            parents,
            facts,
            bounds,
        }
    }

    #[must_use]
    pub fn levels(&self) -> usize {
        self.facts.len()
    }

    #[must_use]
    pub fn cells_on_level(&self, level: usize) -> usize {
        self.facts[level].len()
    }

    /// How wide a key is, in bits. The whole path fits in this many.
    #[must_use]
    pub fn key_bits(&self) -> u32 {
        *self.begins_at.last().expect("a tree has a level")
    }

    /// Where a level's bits begin in a key.
    #[must_use]
    pub fn begins_at(&self, level: usize) -> u32 {
        self.begins_at[level]
    }

    /// What a cell holds.
    #[must_use]
    pub fn facts(&self, level: usize, cell: CellId) -> CellFacts {
        self.facts[level][cell as usize]
    }

    /// The box a cell's nodes lie in.
    #[must_use]
    pub fn bounds(&self, level: usize, cell: CellId) -> &BoundingBox {
        &self.bounds[level][cell as usize]
    }

    /// The cells of the level below that a cell is built out of, and nothing
    /// on the finest level, which is built out of the graph itself.
    #[must_use]
    pub fn children_of(&self, level: usize, cell: CellId) -> &[CellId] {
        if level == 0 {
            return &[];
        }
        let held = &self.children[level - 1];
        let starts = &self.starts[level - 1];
        let from = starts[cell as usize] as usize;
        let to = starts[cell as usize + 1] as usize;
        &held[from..to]
    }

    /// The key of a cell: the path from the root down to it.
    ///
    /// # Panics
    ///
    /// Panics if there is no such cell on that level.
    #[must_use]
    pub fn key_of(&self, level: usize, cell: CellId) -> CellKey {
        assert!(
            (cell as usize) < self.cells_on_level(level),
            "no cell {cell} on level {level}"
        );
        let mut word = u128::from(cell) << self.begins_at[level];
        let mut at = cell;
        for above in level + 1..self.levels() {
            at = self.parent_of(above - 1, at);
            word |= u128::from(at) << self.begins_at[above];
        }
        CellKey {
            word,
            level: u8::try_from(level).expect("more levels than a byte counts"),
        }
    }

    /// The keys a cell's subtree covers, first and last, both included.
    #[must_use]
    pub fn range_of(&self, level: usize, cell: CellId) -> (u128, u128) {
        let key = self.key_of(level, cell);
        (key.first(), key.last(self.begins_at[level]))
    }

    /// The cell of the level above that a cell belongs to.
    ///
    /// # Panics
    ///
    /// Panics on the topmost level, whose cells have nothing above them.
    #[must_use]
    pub fn parent_of(&self, level: usize, cell: CellId) -> CellId {
        self.parents[level][cell as usize]
    }

    /// Refuses a tree written under a version this does not know.
    ///
    /// # Errors
    ///
    /// Returns the version found when it is not the one this reads.
    pub fn check_version(&self) -> Result<(), u16> {
        if self.version == VERSION {
            Ok(())
        } else {
            Err(self.version)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edge::InputEdge, grid_graph::grid_directory, static_graph::StaticGraph};

    /// A square grid of `side` by `side`, cut into the levels the directory
    /// asks for, with a coordinate a node.
    fn grid(side: usize) -> (StaticGraph<u32>, LevelDirectory, Vec<FPCoordinate>) {
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, 1_u32));
                    edges.push(InputEdge::new(node + 1, node, 1_u32));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, 1_u32));
                    edges.push(InputEdge::new(node + side, node, 1_u32));
                }
            }
        }
        let coordinates = (0..side * side)
            .map(|node| {
                FPCoordinate::new(
                    i32::try_from(node / side).unwrap() * 1000,
                    i32::try_from(node % side).unwrap() * 1000,
                )
            })
            .collect();
        (StaticGraph::new(edges), grid_directory(side), coordinates)
    }

    fn tree_of(side: usize) -> (CellTree, PackedPartition, LevelDirectory) {
        let (graph, directory, coordinates) = grid(side);
        let partition = PackedPartition::of(&directory);
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
        (tree, partition, directory)
    }

    #[test]
    fn a_cell_holds_what_the_directory_put_in_it() {
        let (tree, partition, directory) = tree_of(8);
        for level in 0..tree.levels() {
            let mut counted = vec![0_u32; tree.cells_on_level(level)];
            for node in 0..directory.number_of_nodes() {
                counted[partition.cell_in(partition.word(node), level) as usize] += 1;
            }
            for (cell, &held) in counted.iter().enumerate() {
                assert_eq!(
                    tree.facts(level, cell as CellId).nodes,
                    held,
                    "level {level}, cell {cell}"
                );
            }
        }
    }

    #[test]
    fn the_children_of_a_cell_are_the_cells_that_call_it_parent() {
        let (tree, _, directory) = tree_of(8);
        for level in 1..tree.levels() {
            let parents = directory.parents_on_level(level - 1);
            for cell in 0..tree.cells_on_level(level) {
                let wanted = parents
                    .iter()
                    .enumerate()
                    .filter(|&(_, &parent)| parent as usize == cell)
                    .map(|(child, _)| child as CellId)
                    .collect::<Vec<_>>();
                assert_eq!(tree.children_of(level, cell as CellId), wanted);
            }
        }
    }

    #[test]
    fn a_child_names_the_cell_that_holds_it() {
        let (tree, _, _) = tree_of(8);
        for level in 1..tree.levels() {
            for cell in 0..tree.cells_on_level(level) {
                for &child in tree.children_of(level, cell as CellId) {
                    assert_eq!(tree.parent_of(level - 1, child), cell as CellId);
                }
            }
        }
    }

    /// The property the whole format rests on.
    #[test]
    fn a_subtree_is_one_range_of_keys_and_holds_nothing_else() {
        let (tree, partition, directory) = tree_of(8);
        for level in 0..tree.levels() {
            for cell in 0..tree.cells_on_level(level) {
                let (first, last) = tree.range_of(level, cell as CellId);
                assert!(first <= last, "a range runs forwards");
                for node in 0..directory.number_of_nodes() {
                    let word = partition.word(node);
                    let inside = partition.cell_in(word, level) as usize == cell;
                    assert_eq!(
                        inside,
                        (first..=last).contains(&word),
                        "node {node} at level {level}, cell {cell}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_ranges_of_a_level_do_not_overlap_and_come_out_in_order() {
        let (tree, _, _) = tree_of(8);
        for level in 0..tree.levels() {
            let mut ranges = (0..tree.cells_on_level(level))
                .map(|cell| tree.range_of(level, cell as CellId))
                .collect::<Vec<_>>();
            ranges.sort_unstable();
            for pair in ranges.windows(2) {
                assert!(
                    pair[0].1 < pair[1].0,
                    "the range ending {} runs into the one starting {}",
                    pair[0].1,
                    pair[1].0
                );
            }
        }
    }

    #[test]
    fn a_subtree_holds_the_subtrees_below_it() {
        let (tree, _, _) = tree_of(8);
        for level in 1..tree.levels() {
            for cell in 0..tree.cells_on_level(level) {
                let (first, last) = tree.range_of(level, cell as CellId);
                for &child in tree.children_of(level, cell as CellId) {
                    let (from, to) = tree.range_of(level - 1, child);
                    assert!(first <= from && to <= last, "a child falls outside");
                }
            }
        }
    }

    /// A road network is directed, and a node that can only be entered from
    /// another cell is a way in that a path through the cell above may take.
    /// Asking each node about its own outgoing arcs misses every one of them:
    /// on europe.ptv that was 261,684 nodes, nearly a tenth of the border of
    /// the finest level.
    #[test]
    fn a_node_an_arc_only_reaches_is_on_the_border_too() {
        // two cells of two nodes, with one arc running between them one way
        let edges = vec![
            InputEdge::new(0, 1, 1_u32),
            InputEdge::new(1, 0, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 3, 1_u32),
            InputEdge::new(3, 2, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let directory = LevelDirectory::new(vec![0, 0, 1, 1], Vec::new());
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); 4];
        let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

        // node 1 leaves its cell and node 2 is only reached from outside; both
        // are on the border of the cell they are in
        assert_eq!(tree.facts(0, 0).on_border, 1, "the cell the arc leaves");
        assert_eq!(tree.facts(0, 1).on_border, 1, "the cell the arc reaches");
    }

    #[test]
    fn a_tree_reads_back_as_it_was_written() {
        let (tree, _, _) = tree_of(8);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&tree).expect("a tree serializes");
        let read: CellTree =
            rkyv::from_bytes::<CellTree, rkyv::rancor::Error>(&bytes).expect("a tree deserializes");
        assert_eq!(tree, read);
        assert!(read.check_version().is_ok());
    }

    #[test]
    fn a_tree_of_another_version_is_refused() {
        let (mut tree, _, _) = tree_of(4);
        tree.version = VERSION + 1;
        assert_eq!(tree.check_version(), Err(VERSION + 1));
    }
}
