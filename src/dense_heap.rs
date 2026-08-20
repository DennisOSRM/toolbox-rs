//! A queue that finds a node by looking it up in an array rather than in a map.
//!
//! [`crate::addressable_binary_heap::AddressableHeap`] holds the place of each
//! node in a hash map, which is what lets it take a node of any kind that
//! counts. A search over a graph does not need that: its nodes are the numbers
//! zero upwards, so the place of a node can sit in an array at that number and
//! be read without hashing anything.
//!
//! What an array costs is room for every node of the graph whether the search
//! reaches it or not, and the trouble of emptying it. The nodes a run put on
//! the queue are already written down, and a search over the cells of a
//! partition puts a few thousand of them on out of eighteen million, so
//! emptying is a walk of those rather than of the array.
//!
//! The room is the price: four bytes for every node of the graph, seventy odd
//! megabytes on a continent, held for as long as the search object is rather
//! than per run. A search that runs once over a large graph should use the
//! map; a search that runs a great many times, which is what this is for, pays
//! it once and stops paying per node thereafter.

use rustc_hash::FxHashMap;

use crate::{
    graph::NodeID,
    heap_stats::{Counters, HeapStats, Untracked},
};

/// What a table says about a node no run has touched.
pub const MISSING: u32 = u32::MAX;

/// What a queue keeps against a node.
///
/// The two are held together rather than in a table apiece. A queue asks both
/// of the same node in the same breath -- what it has been reached at decides
/// whether to take an offer, and where it sits decides what to do about it --
/// and two tables answer that from two places, which on a graph of millions is
/// two misses where the two numbers would have fitted in one line of cache.
/// Eight bytes a node either way; the difference is only whether they lie
/// beside each other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    /// where the node sits in the queue, and [`MISSING`] for one this run has
    /// not held
    pub place: u32,
    /// What the node is held at now, or was settled at, and [`MISSING`] for
    /// one this run has not reached.
    ///
    /// A search turns an offer away far more often than it takes one, and both
    /// reasons for turning it away -- the node is settled, or it is already
    /// held at no more than this -- are answered by one comparison against
    /// this. A node that has come off the queue keeps what it came off at,
    /// which nothing offered later can beat: a search settles in the order of
    /// what it costs to reach.
    pub best: u32,
}

/// What a table holds for a node nothing has been written for.
pub const UNTOUCHED: Slot = Slot {
    place: MISSING,
    best: MISSING,
};

impl Default for Slot {
    fn default() -> Self {
        UNTOUCHED
    }
}

// Where a queue keeps the four byte numbers it holds against a node.
//
// A queue asks two things of every node it meets: where it sits in the queue,
// and what it has been reached at. Both are read far more often than they are
// written, and how to keep them is a trade rather than an answer. An array over
// the whole graph answers in one look and costs room for every node whether the
// run reaches it or not; a map costs a hash on every look and only room for
// what was reached.
//
// Which way round that trade falls is a property of the search, not of the
// queue: a search run once over a continent should not allocate seventy
// megabytes to touch a thousand nodes, and a search run a million times should
// not hash on every arc it relaxes. So each search says which it wants, by
// naming the queue built over it. This is what OSRM does with the
// `IndexStorage` its query heap is written against.
//
// The two tables below carry no trait between them. They answer to the same
// three names, and `query_heap!` builds a queue over whichever it is handed,
// so the choice is made by expansion rather than by a type parameter.

/// A table over the numbers of the graph, read in one look.
///
/// The room is the price: four bytes for every node whether the search reaches
/// it or not, held for as long as the queue is rather than per run. A search
/// that runs a great many times pays it once and stops paying per node.
#[derive(Default)]
pub struct ByArray {
    of_node: Vec<Slot>,
}

impl ByArray {
    #[inline]
    pub fn get(&self, node: NodeID) -> Slot {
        self.of_node.get(node).copied().unwrap_or(UNTOUCHED)
    }

    #[inline]
    pub fn set(&mut self, node: NodeID, slot: Slot) {
        // The write is the hot path and growing is not, so the two are kept
        // apart: a call to grow sitting in here is enough to stop the whole of
        // it being inlined into the search.
        if let Some(held) = self.of_node.get_mut(node) {
            *held = slot;
            return;
        }
        self.grow_to_hold(node, slot);
    }

    #[inline]
    pub fn reset(&mut self, node: NodeID) {
        if let Some(held) = self.of_node.get_mut(node) {
            *held = UNTOUCHED;
        }
    }
}

impl ByArray {
    /// Makes room for a node beyond what has been asked about so far, which
    /// happens once per node of the graph and never again.
    #[cold]
    #[inline(never)]
    fn grow_to_hold(&mut self, node: NodeID, slot: Slot) {
        self.of_node.resize(node + 1, UNTOUCHED);
        self.of_node[node] = slot;
    }
}

/// A table that holds only what was written to it.
///
/// A hash on every look, against room for what the run reached rather than for
/// the graph. This is what a search that runs once over a large graph wants,
/// and what one that runs a great many times does not.
#[derive(Default)]
pub struct ByMap {
    of_node: FxHashMap<NodeID, Slot>,
}

impl ByMap {
    #[inline]
    pub fn get(&self, node: NodeID) -> Slot {
        self.of_node.get(&node).copied().unwrap_or(UNTOUCHED)
    }

    #[inline]
    pub fn set(&mut self, node: NodeID, slot: Slot) {
        self.of_node.insert(node, slot);
    }

    #[inline]
    pub fn reset(&mut self, node: NodeID) {
        // taken out rather than written over, or the map would keep growing
        // with every node any run has ever reached
        self.of_node.remove(&node);
    }
}

/// A queue over dense node ids that counts nothing.
pub type DenseQueue = DenseHeap<Untracked>;

/// The same queue, counting what it was asked to do.
pub type TrackedDenseQueue = DenseHeap<Counters>;

/// What the queue keeps beside a node it has held.
///
/// Every field is four bytes rather than eight. The queue reads one of these
/// for every arc a search relaxes, and stepping over a cell relaxes the whole
/// border of it, so this array is streamed rather than dipped into. Eight byte
/// fields made it thirty two bytes a node where sixteen will do, which is
/// twice the memory to move for the same answers. A node id and a place in the
/// heap are both bounded by the size of the graph, and the crate cannot hold a
/// graph of more than four thousand million nodes anyway: the cell tables
/// address it with four bytes. A weight is bounded the same way, by the same
/// argument [`CellDistances`](crate::customization::CellDistances) makes for
/// the tables. Measured over a continent, the longest way across it by the
/// clock came to a million and a third, which is a three thousandth of what
/// four bytes reach.
#[derive(Clone, Copy)]
struct Held {
    node: u32,
    /// where in the heap this node sits, and zero once it has come off for good
    key: u32,
    weight: u32,
    data: u32,
}

/// The place of a node that has not been on the queue during this run.
const NOWHERE: u32 = MISSING;

/// Builds a queue over a given way of keeping what it knows about a node.
///
/// The storage is chosen by expanding this rather than by a type parameter.
/// Both say the same thing to a caller, and a generic over a trait is the
/// tidier of the two; what the macro buys is that each queue is a plain struct
/// holding a plain table, with nothing for the compiler to see through. Whether
/// that is worth the loss of tidiness is a question for a measurement, not for
/// an opinion -- see the note on the two queues below.
macro_rules! query_heap {
    ($name:ident, $table:ty, $what:literal) => {
        #[doc = $what]
        pub struct $name<S: HeapStats<NodeID> = Untracked> {
            /// the binary heap itself, as places into `held` against what each is held
            /// at, one based so that the root has a parent slot to stop at
            heap: Vec<(u32, u32)>,
            held: Vec<Held>,
            /// where each node sits in `held` and what it has been reached at, both
            /// answered by one look
            table: $table,
            stats: S,
        }

        impl<S: HeapStats<NodeID>> Default for $name<S> {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<S: HeapStats<NodeID>> $name<S> {
            #[must_use]
            pub fn new() -> Self {
                Self {
                    heap: vec![(0, 0)],
                    held: Vec::new(),
                    table: <$table>::default(),
                    stats: S::default(),
                }
            }

            pub fn stats(&self) -> &S {
                &self.stats
            }

            /// Forgets the run that has just finished.
            ///
            /// Only what that run put on the queue is put back, which is a few
            /// thousand nodes of a graph of millions.
            pub fn clear(&mut self) {
                self.heap.truncate(1);
                for held in &self.held {
                    self.table.reset((held.node as usize).try_into().unwrap());
                }
                self.held.clear();
                self.stats = S::default();
            }

            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.heap.len() <= 1
            }

            /// How many nodes the queue held during this run.
            #[must_use]
            pub fn inserted_len(&self) -> usize {
                self.held.len()
            }

            #[inline]
            fn place_of(&self, node: NodeID) -> Option<usize> {
                match self.table.get(node).place {
                    NOWHERE => None,
                    place => Some(place as usize),
                }
            }

            /// Whether the node has been on the queue during this run.
            #[must_use]
            pub fn inserted(&self, node: NodeID) -> bool {
                self.place_of(node).is_some()
            }

            /// Whether the node is on the queue now.
            #[must_use]
            pub fn contains(&self, node: NodeID) -> bool {
                self.place_of(node)
                    .is_some_and(|place| self.held[place].key != 0)
            }

            /// What the node is held at, and the largest number there is for one the
            /// queue has never held.
            #[must_use]
            #[inline]
            pub fn weight(&self, node: NodeID) -> usize {
                self.place_of(node)
                    .map_or(usize::MAX, |place| self.held[place].weight as usize)
            }

            /// What was written down beside the node, which a search uses for the node
            /// it was reached from.
            ///
            /// # Panics
            ///
            /// Panics for a node the queue has not held during this run.
            #[must_use]
            pub fn data(&self, node: NodeID) -> NodeID {
                let place = self.place_of(node).expect("the queue has not held that");
                (self.held[place].data as usize).try_into().unwrap()
            }

            #[inline]
            pub fn insert(&mut self, node: NodeID, weight: usize, data: NodeID) {
                let place = self.held.len();
                let key = self.heap.len();
                let offered = u32::try_from(weight).unwrap_or(u32::MAX);
                self.held.push(Held {
                    node: u32::try_from(node).expect("the graph is too large to hold"),
                    key: u32::try_from(key).expect("too many nodes on one queue"),
                    weight: offered,
                    data: u32::try_from(data).expect("the graph is too large to hold"),
                });
                self.table.set(
                    node,
                    Slot {
                        place: u32::try_from(place).expect("too many nodes on one queue"),
                        best: offered,
                    },
                );
                self.heap.push((
                    u32::try_from(place).expect("too many nodes on one queue"),
                    offered,
                ));
                self.stats.inserted(node);
                self.up_heap(key);
            }

            /// Puts a node on the queue, or lowers what it is held at, in one look.
            #[inline]
            pub fn insert_or_decrease(
                &mut self,
                node: NodeID,
                weight: usize,
                data: NodeID,
            ) -> bool {
                let offered = u32::try_from(weight).unwrap_or(u32::MAX);
                // One look answers everything asked of the table here: both ways of
                // turning an offer away, and where the node sits if it is taken. A
                // node no run has reached reads back as [`UNTOUCHED`], whose weight is
                // the largest there is, so nothing a search really offers is turned
                // away by it.
                let slot = self.table.get(node);
                if offered >= slot.best {
                    return false;
                }
                if slot.place == NOWHERE {
                    self.insert(node, weight, data);
                    return true;
                }
                let place = slot.place as usize;
                let held = self.held[place];
                // A node that has come off the queue is turned away by the comparison
                // above, as it came off at no more than anything offered afterwards.
                // Except by an offer smaller than what it came off at, which a search
                // that settles in order never makes and which this refuses anyway.
                if held.key == 0 {
                    return false;
                }
                self.held[place].weight = offered;
                self.held[place].data =
                    u32::try_from(data).expect("the graph is too large to hold");
                self.table.set(
                    node,
                    Slot {
                        place: slot.place,
                        best: offered,
                    },
                );
                self.heap[held.key as usize].1 = offered;
                self.stats.decreased(node);
                self.up_heap(held.key as usize);
                true
            }

            /// What the lightest node is held at, without taking it off.
            ///
            /// A search that runs from both ends asks this of each side once per
            /// settled node, to know whether the two fronts have met.
            ///
            /// # Panics
            ///
            /// Panics on an empty queue.
            #[must_use]
            pub fn min_weight(&self) -> usize {
                assert!(!self.is_empty(), "the queue is empty");
                self.heap[1].1 as usize
            }

            /// Takes the lightest node off the queue.
            ///
            /// # Panics
            ///
            /// Panics on an empty queue.
            #[inline]
            pub fn delete_min(&mut self) -> NodeID {
                assert!(!self.is_empty(), "the queue is empty");
                let place = self.heap[1].0 as usize;
                let last = self.heap.len() - 1;
                self.heap.swap(1, last);
                self.heap.pop();
                if self.heap.len() > 1 {
                    self.held[self.heap[1].0 as usize].key = 1;
                    self.down_heap(1);
                }
                self.held[place].key = 0;
                let node = self.held[place].node as usize;
                self.stats.deleted(node.try_into().unwrap());
                node.try_into().unwrap()
            }

            #[inline]
            fn up_heap(&mut self, mut key: usize) {
                let rising = self.heap[key];
                while key > 1 {
                    let parent = key >> 1;
                    if self.heap[parent].1 <= rising.1 {
                        break;
                    }
                    self.heap[key] = self.heap[parent];
                    self.held[self.heap[key].0 as usize].key = key as u32;
                    key = parent;
                }
                self.heap[key] = rising;
                self.held[rising.0 as usize].key = key as u32;
            }

            #[inline]
            fn down_heap(&mut self, mut key: usize) {
                let sinking = self.heap[key];
                loop {
                    let mut child = key << 1;
                    if child >= self.heap.len() {
                        break;
                    }
                    if child + 1 < self.heap.len() && self.heap[child + 1].1 < self.heap[child].1 {
                        child += 1;
                    }
                    if self.heap[child].1 >= sinking.1 {
                        break;
                    }
                    self.heap[key] = self.heap[child];
                    self.held[self.heap[key].0 as usize].key = key as u32;
                    key = child;
                }
                self.heap[key] = sinking;
                self.held[sinking.0 as usize].key = key as u32;
            }
        }
    };
}

query_heap!(
    DenseHeap,
    ByArray,
    "A queue that finds a node in an array, in one look and at the price of \
     room for every node of the graph."
);
query_heap!(
    HashHeap,
    ByMap,
    "A queue that finds a node in a map, at the price of a hash on every look \
     and only room for what a run reached."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lightest_comes_off_first() {
        let mut queue = DenseQueue::new();
        for (node, weight) in [(3, 30), (1, 10), (2, 20)] {
            queue.insert(node, weight, node);
        }
        assert_eq!(queue.delete_min(), 1);
        assert_eq!(queue.delete_min(), 2);
        assert_eq!(queue.delete_min(), 3);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_node_lowered_comes_off_sooner() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        queue.insert(2, 20, 2);
        assert!(queue.insert_or_decrease(2, 5, 9));
        assert_eq!(queue.delete_min(), 2);
        assert_eq!(queue.weight(2), 5);
        assert_eq!(queue.data(2), 9);
    }

    #[test]
    fn a_node_is_not_lowered_to_something_larger() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        assert!(!queue.insert_or_decrease(1, 20, 7));
        assert_eq!(queue.weight(1), 10);
        assert_eq!(queue.data(1), 1);
    }

    /// A node that has come off for good stays off, whatever is offered for it
    /// afterwards.
    #[test]
    fn a_node_that_has_come_off_does_not_go_back_on() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        assert_eq!(queue.delete_min(), 1);
        assert!(!queue.insert_or_decrease(1, 1, 1));
        assert!(queue.inserted(1));
        assert!(!queue.contains(1));
        assert_eq!(queue.weight(1), 10);
    }

    /// What one run held is not what the next one sees, and the forgetting is
    /// a count rather than a walk.
    #[test]
    fn a_run_does_not_see_what_the_one_before_it_held() {
        let mut queue = DenseQueue::new();
        queue.insert(5, 50, 5);
        assert!(queue.inserted(5));

        queue.clear();
        assert!(!queue.inserted(5));
        assert!(!queue.contains(5));
        assert_eq!(queue.weight(5), usize::MAX);
        assert_eq!(queue.inserted_len(), 0);
    }

    /// A run puts back what it held and nothing else, so a node of a graph
    /// the run never reached is untouched by it.
    #[test]
    fn a_run_puts_back_only_what_it_held() {
        let mut queue = DenseQueue::new();
        queue.insert(9, 90, 9);
        queue.insert(2, 20, 2);
        queue.clear();

        assert!(!queue.inserted(9));
        assert!(!queue.inserted(2));
        assert!(!queue.inserted(5), "a node it never held");
        queue.insert(9, 1, 9);
        assert_eq!(queue.weight(9), 1);
    }

    /// The storage is a parameter, so the two have to answer alike. A queue
    /// over a map that disagreed with one over an array would be a search that
    /// gives different distances for the same graph depending on which table
    /// it happened to be built with.
    #[test]
    fn the_two_storages_answer_alike() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_5707);
        for round in 0..12 {
            let count = 40 + round * 11;
            let mut dense = DenseQueue::new();
            let mut hashed = HashHeap::<Untracked>::new();
            for node in 0..count {
                let weight = rng.random_range(0..500_usize);
                dense.insert(node, weight, node);
                hashed.insert(node, weight, node);
            }
            for node in 0..count {
                let lower = rng.random_range(0..600_usize);
                assert_eq!(
                    dense.insert_or_decrease(node, lower, node + 1),
                    hashed.insert_or_decrease(node, lower, node + 1),
                    "round {round}, node {node}"
                );
            }
            while !dense.is_empty() {
                assert!(!hashed.is_empty());
                let from_dense = dense.delete_min();
                let from_hashed = hashed.delete_min();
                assert_eq!(from_dense, from_hashed, "round {round}");
                assert_eq!(dense.weight(from_dense), hashed.weight(from_hashed));
                assert_eq!(dense.data(from_dense), hashed.data(from_hashed));
            }
            assert!(hashed.is_empty());
        }
    }

    /// A map keeps only what was written to it, so a run has to leave it empty
    /// or it grows with every node any run has ever reached.
    #[test]
    fn a_map_holds_nothing_once_a_run_is_forgotten() {
        let mut queue = HashHeap::<Untracked>::new();
        for node in 0..64 {
            queue.insert(node, node * 3, node);
        }
        queue.delete_min();
        queue.clear();
        assert_eq!(
            queue.table.of_node.len(),
            0,
            "the table was left holding nodes"
        );
        assert!(!queue.inserted(7));
        assert_eq!(queue.weight(7), usize::MAX);
    }

    #[test]
    fn a_heap_of_many_comes_out_in_order() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_DE95);
        for round in 0..20 {
            let mut queue = DenseQueue::new();
            let count = 50 + round * 7;
            let mut weights = Vec::new();
            for node in 0..count {
                let weight = rng.random_range(0..1000_usize);
                weights.push(weight);
                queue.insert(node, weight, node);
            }
            // and half of them lowered afterwards
            for (node, weight) in weights.iter_mut().enumerate() {
                if rng.random_range(0..2) == 0 {
                    let lower = rng.random_range(0..(*weight).max(1));
                    if queue.insert_or_decrease(node, lower, node) {
                        *weight = lower;
                    }
                }
            }

            weights.sort_unstable();
            let mut came_out = Vec::new();
            while !queue.is_empty() {
                let node = queue.delete_min();
                came_out.push(queue.weight(node));
            }
            assert_eq!(came_out, weights, "round {round}");
        }
    }
}
