//! A run of nodes' arcs, packed small enough to be worth reading one at a time.
//!
//! # Why the graph pages at all
//!
//! The cell tables were the first thing paged because they were the thing this
//! crate had just built. They are not the big thing: on a continent the arcs
//! come to four hundred mebibytes against tables that run in a tenth of that,
//! so an instance that pages only its tables is still an instance that holds
//! four hundred mebibytes of graph whatever budget it was given.
//!
//! # What makes a block small
//!
//! Three things, and the numbering is what buys all three.
//!
//! The nodes were renumbered so that a cell's nodes are a run, which is what
//! let a cell table be found by a range. The same run means an arc mostly ends
//! near where it began: a road leaves a node for one a few hundred numbers
//! away, not for one ten million away. So a target is written as the signed
//! step from its own source, zigzagged, and the block takes as many bits as its
//! widest step wants rather than the twenty five a continent's node numbers
//! want.
//!
//! A weight is bounded by what a weight is, and the widest in a block is
//! usually far under the widest in the graph, so it takes the bits its own
//! block wants too.
//!
//! And the degrees: a node has a handful of arcs, so the count is three or four
//! bits and the offsets are read by adding them up rather than stored.
//!
//! # What is not here
//!
//! No compression: a block is bit-packed and then handed to whichever
//! [`Codec`](crate::block_codec::Codec) the writer was told to use, exactly as
//! a cell block is. The two go through the same store.

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    graph::NodeID,
    packed_distances::{bits_for, read_at, write_at},
};

/// The version this is written under.
pub const VERSION: u16 = 1;

/// Zigzag, so a step backwards costs what the same step forwards does.
#[inline]
#[must_use]
pub fn zigzag(step: i64) -> u64 {
    ((step << 1) ^ (step >> 63)) as u64
}

/// And back again.
#[inline]
#[must_use]
pub fn unzigzag(held: u64) -> i64 {
    ((held >> 1) as i64) ^ -((held & 1) as i64)
}

/// The arcs of a run of nodes.
///
/// The nodes are `first_node ..  first_node + degrees.len()`, and their arcs
/// are `first_edge ..` in the same order, so a caller that knows a node's
/// number knows where its arcs are without a lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct GraphBlock {
    version: u16,
    /// the first node of the run, and the first arc of the first node
    first_node: u32,
    first_edge: u64,
    /// how many nodes, and how many arcs in all
    nodes: u32,
    edges: u32,
    /// what each of the three packed runs takes an entry
    degree_bits: u32,
    step_bits: u32,
    weight_bits: u32,
    border_bits: u32,
    /// the degrees, then the zigzagged steps, then the weights, then the
    /// levels at which each arc leaves the cell of its source
    packed: Vec<u8>,
}

impl GraphBlock {
    /// Packs the arcs of a run of nodes.
    ///
    /// `arcs` is one entry a node, in node order, each the arcs of that node as
    /// (target, weight, border level).
    ///
    /// The border level is one past the highest level at which the arc leaves
    /// the cell of its source, and zero for one that leaves none -- the same
    /// byte [`BorderLevels`](crate::border_levels::BorderLevels) holds. It
    /// rides with the arc because that is what it is a property of, and
    /// because a search wants it at exactly the moment it has the arc: kept
    /// apart it is a byte an arc standing resident, which on a continent is
    /// forty megabytes to say what three bits an arc say here.
    ///
    /// # Panics
    ///
    /// Panics where a step does not fit in sixty three bits, which no graph
    /// this crate can hold produces.
    #[must_use]
    pub fn of(first_node: u32, first_edge: u64, arcs: &[Vec<(u32, u32, u8)>]) -> Self {
        let edges: usize = arcs.iter().map(Vec::len).sum();
        let widest_degree = arcs.iter().map(Vec::len).max().unwrap_or(0);
        let widest_step = arcs
            .iter()
            .enumerate()
            .flat_map(|(at, out)| {
                let source = i64::from(first_node) + at as i64;
                out.iter()
                    .map(move |&(target, _, _)| zigzag(i64::from(target) - source))
            })
            .max()
            .unwrap_or(0);
        let widest_weight = arcs
            .iter()
            .flat_map(|out| out.iter().map(|&(_, weight, _)| weight))
            .max()
            .unwrap_or(0);
        let widest_border = arcs
            .iter()
            .flat_map(|out| out.iter().map(|&(_, _, border)| u32::from(border)))
            .max()
            .unwrap_or(0);

        let degree_bits = bits_for(u32::try_from(widest_degree).expect("a degree in four bytes"));
        let step_bits = bits_for_wide(widest_step);
        let weight_bits = bits_for(widest_weight);
        let border_bits = bits_for(widest_border);

        let bits = arcs.len() * degree_bits as usize
            + edges * (step_bits + weight_bits + border_bits) as usize;
        let mut packed = vec![0_u8; bits.div_ceil(8) + 8];

        let mut at = 0;
        for out in arcs {
            write_at(
                &mut packed,
                at,
                degree_bits,
                u32::try_from(out.len()).expect("a degree in four bytes"),
            );
            at += degree_bits as usize;
        }
        for (index, out) in arcs.iter().enumerate() {
            let source = i64::from(first_node) + index as i64;
            for &(target, _, _) in out {
                write_wide(
                    &mut packed,
                    at,
                    step_bits,
                    zigzag(i64::from(target) - source),
                );
                at += step_bits as usize;
            }
        }
        for out in arcs {
            for &(_, weight, _) in out {
                write_at(&mut packed, at, weight_bits, weight);
                at += weight_bits as usize;
            }
        }
        for out in arcs {
            for &(_, _, border) in out {
                write_at(&mut packed, at, border_bits, u32::from(border));
                at += border_bits as usize;
            }
        }

        Self {
            version: VERSION,
            first_node,
            first_edge,
            nodes: u32::try_from(arcs.len()).expect("a run in four bytes"),
            edges: u32::try_from(edges).expect("a run of arcs in four bytes"),
            degree_bits,
            step_bits,
            weight_bits,
            border_bits,
            packed,
        }
    }

    #[must_use]
    pub fn first_node(&self) -> NodeID {
        self.first_node as NodeID
    }

    #[must_use]
    pub fn nodes(&self) -> usize {
        self.nodes as usize
    }

    #[must_use]
    pub fn first_edge(&self) -> u64 {
        self.first_edge
    }

    #[must_use]
    pub fn edges(&self) -> usize {
        self.edges as usize
    }

    /// Whether a node's arcs are in this block.
    #[must_use]
    pub fn holds(&self, node: NodeID) -> bool {
        node >= self.first_node as NodeID && node < (self.first_node as NodeID + self.nodes())
    }

    /// Whether an arc is in this block.
    #[must_use]
    pub fn holds_edge(&self, edge: u64) -> bool {
        edge >= self.first_edge && edge < self.first_edge + u64::from(self.edges)
    }

    /// Reads the block back into the arrays a search walks.
    ///
    /// The packing is for the file. What a store holds is bounded by what it
    /// holds unpacked, exactly as it is for a cell table, and a search that had
    /// to unpick a bit field per arc it relaxes would pay for the file on every
    /// step of every query.
    pub fn unpack_into(&self, held: &mut HeldArcs) {
        held.first_node = self.first_node;
        held.first_edge = self.first_edge;

        held.starts.clear();
        held.starts.reserve(self.nodes as usize + 1);
        let mut seen = 0_u32;
        held.starts.push(0);
        for node in 0..self.nodes as usize {
            seen += read_at(
                &self.packed,
                node * self.degree_bits as usize,
                self.degree_bits,
            );
            held.starts.push(seen);
        }

        held.targets.clear();
        held.targets.reserve(self.edges as usize);
        let steps_at = self.nodes as usize * self.degree_bits as usize;
        for node in 0..self.nodes as usize {
            let source = i64::from(self.first_node) + node as i64;
            for place in held.starts[node]..held.starts[node + 1] {
                let step = unzigzag(read_wide(
                    &self.packed,
                    steps_at + place as usize * self.step_bits as usize,
                    self.step_bits,
                ));
                held.targets.push((source + step) as u32);
            }
        }

        held.weights.clear();
        held.weights.reserve(self.edges as usize);
        let weights_at = steps_at + self.edges as usize * self.step_bits as usize;
        for place in 0..self.edges as usize {
            held.weights.push(read_at(
                &self.packed,
                weights_at + place * self.weight_bits as usize,
                self.weight_bits,
            ));
        }

        held.borders.clear();
        held.borders.reserve(self.edges as usize);
        let borders_at = weights_at + self.edges as usize * self.weight_bits as usize;
        for place in 0..self.edges as usize {
            held.borders.push(
                u8::try_from(read_at(
                    &self.packed,
                    borders_at + place * self.border_bits as usize,
                    self.border_bits,
                ))
                .expect("a level in a byte"),
            );
        }
    }

    /// What the block takes once read back.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>() + self.packed.capacity()
    }

    /// Refuses a block written under a version this does not know.
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

/// How many bits a value of up to sixty four wants.
fn bits_for_wide(largest: u64) -> u32 {
    if largest == 0 {
        1
    } else {
        64 - largest.leading_zeros()
    }
}

/// The same as [`write_at`], for a value that may want more than thirty two
/// bits: a step across a continent does, where a weight and a degree do not.
fn write_wide(packed: &mut [u8], at: usize, bits: u32, value: u64) {
    let low = bits.min(32);
    write_at(packed, at, low, (value & 0xFFFF_FFFF) as u32);
    if bits > 32 {
        write_at(packed, at + low as usize, bits - 32, (value >> 32) as u32);
    }
}

fn read_wide(packed: &[u8], at: usize, bits: u32) -> u64 {
    let low = bits.min(32);
    let mut held = u64::from(read_at(packed, at, low));
    if bits > 32 {
        held |= u64::from(read_at(packed, at + low as usize, bits - 32)) << 32;
    }
    held
}

/// A block of arcs as a search walks them.
///
/// Plain arrays: the offsets a node's arcs begin at, and a target and a weight
/// each. Eight bytes an arc, which is what the same arcs cost in a graph held
/// whole -- the saving is on the file and in how few of these are held at once,
/// not in what one of them takes.
#[derive(Clone, Debug, Default)]
pub struct HeldArcs {
    first_node: u32,
    first_edge: u64,
    /// one more than the nodes, so a node's arcs are `starts[i]..starts[i + 1]`
    starts: Vec<u32>,
    targets: Vec<u32>,
    weights: Vec<u32>,
    /// one past the highest level each arc leaves its source's cell at, and
    /// zero for one that leaves none
    borders: Vec<u8>,
}

impl HeldArcs {
    #[must_use]
    pub fn first_node(&self) -> NodeID {
        self.first_node as NodeID
    }

    #[must_use]
    pub fn nodes(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    #[must_use]
    pub fn first_edge(&self) -> u64 {
        self.first_edge
    }

    #[must_use]
    pub fn edges(&self) -> usize {
        self.targets.len()
    }

    /// Whether a node's arcs are here.
    #[must_use]
    pub fn holds(&self, node: NodeID) -> bool {
        node >= self.first_node as NodeID && node < self.first_node as NodeID + self.nodes()
    }

    /// Whether an arc is here.
    #[must_use]
    pub fn holds_edge(&self, edge: u64) -> bool {
        edge >= self.first_edge && edge < self.first_edge + self.edges() as u64
    }

    /// Where a node's arcs begin and end, as numbers of the whole graph.
    #[must_use]
    pub fn range_of(&self, node: NodeID) -> (u64, u64) {
        debug_assert!(self.holds(node));
        let place = node - self.first_node as NodeID;
        (
            self.first_edge + u64::from(self.starts[place]),
            self.first_edge + u64::from(self.starts[place + 1]),
        )
    }

    /// Where an arc goes, and nothing where it is not here.
    #[must_use]
    pub fn target(&self, edge: u64) -> Option<NodeID> {
        self.holds_edge(edge)
            .then(|| self.targets[(edge - self.first_edge) as usize] as NodeID)
    }

    /// What an arc costs, and nothing where it is not here.
    #[must_use]
    pub fn weight(&self, edge: u64) -> Option<u32> {
        self.holds_edge(edge)
            .then(|| self.weights[(edge - self.first_edge) as usize])
    }

    /// Whether an arc leaves the cell its source sits in at this level.
    ///
    /// Cells nest, so an arc parting at some level parts at every level below
    /// it and one comparison answers for the level asked about.
    #[must_use]
    pub fn leaves_cell(&self, edge: u64, level: usize) -> Option<bool> {
        self.holds_edge(edge)
            .then(|| usize::from(self.borders[(edge - self.first_edge) as usize]) > level)
    }

    /// What this takes.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + self.starts.capacity() * size_of::<u32>()
            + self.targets.capacity() * size_of::<u32>()
            + self.weights.capacity() * size_of::<u32>()
            + self.borders.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arcs with steps both ways and a wide spread, since the whole of the
    /// packing rests on a step being small and it has to hold when one is not.
    fn arcs(first_node: u32, nodes: usize) -> Vec<Vec<(u32, u32, u8)>> {
        (0..nodes)
            .map(|at| {
                let source = first_node + at as u32;
                // a border level apiece, some staying inside and some not
                let mut out = vec![
                    (source + 1, 10 + at as u32, (at % 4) as u8),
                    (
                        source.saturating_sub(1),
                        20 + at as u32,
                        ((at + 1) % 3) as u8,
                    ),
                ];
                // one arc a long way off, which is what sets the block's width
                if at == nodes / 2 {
                    out.push((source + 100_000, 7, 5));
                }
                // and a node with nothing leaving it
                if at == 1 {
                    out.clear();
                }
                out
            })
            .collect()
    }

    #[test]
    fn zigzag_goes_both_ways() {
        for step in [-1_000_000_i64, -3, -1, 0, 1, 3, 1_000_000] {
            assert_eq!(unzigzag(zigzag(step)), step, "step {step}");
        }
        // a step backwards costs what the same step forwards does
        assert_eq!(bits_for_wide(zigzag(-5)), bits_for_wide(zigzag(5)));
    }

    #[test]
    fn a_block_reads_back_the_arcs_it_was_given() {
        let first_node = 4_000;
        let out = arcs(first_node, 64);
        let block = GraphBlock::of(first_node, 900, &out);
        let mut held = HeldArcs::default();
        block.unpack_into(&mut held);

        assert_eq!(held.nodes(), 64);
        assert_eq!(held.edges(), out.iter().map(Vec::len).sum::<usize>());
        assert_eq!(held.first_edge(), 900);

        let mut edge = 900;
        for (at, wanted) in out.iter().enumerate() {
            let node = first_node as NodeID + at;
            let (from, upto) = held.range_of(node);
            assert_eq!(from, edge, "node {node} begins elsewhere");
            assert_eq!((upto - from) as usize, wanted.len(), "node {node} degree");
            for &(target, weight, border) in wanted {
                assert_eq!(held.target(edge), Some(target as NodeID));
                assert_eq!(held.weight(edge), Some(weight));
                // the level rides with the arc: it parts at every level under
                // the one it was given, and at none from there up
                for level in 0..8 {
                    assert_eq!(
                        held.leaves_cell(edge, level),
                        Some(usize::from(border) > level),
                        "arc {edge} at level {level}"
                    );
                }
                edge += 1;
            }
        }
        assert_eq!(edge, 900 + held.edges() as u64);
    }

    #[test]
    fn an_arc_of_another_block_is_not_answered_for() {
        let block = GraphBlock::of(0, 0, &arcs(0, 8));
        let mut held = HeldArcs::default();
        block.unpack_into(&mut held);
        let past = held.first_edge() + held.edges() as u64;
        assert_eq!(held.target(past), None);
        assert_eq!(held.weight(past), None);
        assert!(!held.holds(held.nodes()));
    }

    #[test]
    fn a_run_of_nothing_packs_and_reads_back_as_nothing() {
        let block = GraphBlock::of(7, 21, &vec![Vec::<(u32, u32, u8)>::new(); 4]);
        let mut held = HeldArcs::default();
        block.unpack_into(&mut held);
        assert_eq!(held.nodes(), 4);
        assert_eq!(held.edges(), 0);
        for node in 7..11 {
            assert_eq!(held.range_of(node), (21, 21));
        }
    }

    /// The point of the steps: a block whose arcs stay near home takes far
    /// fewer bits than the node numbers themselves want.
    #[test]
    fn arcs_that_stay_near_home_pack_smaller_than_their_node_numbers() {
        let near: Vec<Vec<(u32, u32, u8)>> = (0..256)
            .map(|at| vec![(9_000_000 + at + 1, 5, 1), (9_000_000 + at + 2, 5, 1)])
            .collect();
        let far: Vec<Vec<(u32, u32, u8)>> = (0..256)
            .map(|at| {
                vec![
                    (at * 70_001 % 17_000_000, 5, 1),
                    (at * 31 % 17_000_000, 5, 1),
                ]
            })
            .collect();
        let near = GraphBlock::of(9_000_000, 0, &near);
        let far = GraphBlock::of(9_000_000, 0, &far);
        assert!(
            near.packed.len() * 3 < far.packed.len(),
            "near {} bytes, far {} bytes",
            near.packed.len(),
            far.packed.len()
        );
    }

    #[test]
    fn a_block_reads_back_as_it_was_written() {
        let block = GraphBlock::of(1_000, 55, &arcs(1_000, 32));
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&block).expect("serializes");
        let read: GraphBlock =
            rkyv::from_bytes::<GraphBlock, rkyv::rancor::Error>(&bytes).expect("deserializes");
        assert_eq!(read, block);
        assert!(read.check_version().is_ok());
    }
}
