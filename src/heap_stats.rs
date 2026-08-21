//! What a search did, for whoever wants to know, and nothing at all for
//! whoever does not.
//!
//! A search that is being measured for speed and a search that is being asked
//! what it did are two different runs. Counting costs something, and a run
//! that carries counters is not the run whose time is worth reporting, so the
//! collecting is a type that is given rather than a switch that is read.
//! [`Untracked`] holds nothing and does nothing, and a queue built on it is
//! the same machine it was before any of this existed.
//!
//! The counting sits on the queue rather than on the search that drives it.
//! Everything worth counting is something the queue is asked to do, the queue
//! is asked to do all of it in one place, and a search that primes its queue
//! outside of its own loop cannot then forget to count that one.
//!
//! # Examples
//!
//! ```rust
//! use toolbox_rs::edge::InputEdge;
//! use toolbox_rs::static_graph::StaticGraph;
//! use toolbox_rs::unidirectional_dijkstra::{
//!     TrackedUnidirectionalDijkstra, UnidirectionalDijkstra,
//! };
//!
//! let graph = StaticGraph::new(vec![
//!     InputEdge::new(0, 1, 1_u32),
//!     InputEdge::new(1, 2, 1_u32),
//! ]);
//!
//! // the plain search carries nothing
//! let mut plain = UnidirectionalDijkstra::new();
//! assert_eq!(plain.run(&graph, 0, 2), 2);
//!
//! // and the tracked one says what it did, built the same way
//! let mut counted = TrackedUnidirectionalDijkstra::new();
//! assert_eq!(counted.run(&graph, 0, 2), 2);
//! assert_eq!(counted.stats().deleted, 3);
//! ```

use crate::graph::NodeID;

/// What a queue tells whoever is collecting.
///
/// The three events are the three things that happen to a node on a queue,
/// named for what the queue does rather than for what a search makes of it.
/// A Dijkstra over an addressable queue settles a node exactly when the queue
/// deletes it, as nothing stale is ever left on it to be thrown away, but that
/// is the search's claim about its own queue and not something the queue says
/// about itself.
///
/// The node is handed over with each event, which is what lets [`RankTargets`]
/// say which node sits at a rank rather than only how many there were.
pub trait HeapStats<Node>: Default {
    /// the node has gone onto the queue for the first time
    fn inserted(&mut self, node: Node);

    /// the node has come off the queue for good
    fn deleted(&mut self, node: Node);

    /// a smaller weight has been found for a node already on the queue
    fn decreased(&mut self, node: Node);
}

/// Collects nothing.
///
/// This is what a run that is being timed is built on. Every method is empty
/// and the type holds no data, so what is left after the compiler has been
/// through it is the search on its own.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Untracked;

impl<Node> HeapStats<Node> for Untracked {
    #[inline]
    fn inserted(&mut self, _node: Node) {}

    #[inline]
    fn deleted(&mut self, _node: Node) {}

    #[inline]
    fn decreased(&mut self, _node: Node) {}
}

/// How many of each, and nothing about which.
///
/// The deleted count is the one a Dijkstra rank is measured in: the rank of a
/// target is how many nodes were settled before it, and a search over this
/// queue settles a node when the queue deletes it. The inserted count is the
/// frontier the search touched, which on a road network runs several times
/// larger, and the two are worth keeping apart.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Counters {
    pub inserted: usize,
    pub deleted: usize,
    pub decreased: usize,
}

impl<Node> HeapStats<Node> for Counters {
    #[inline]
    fn inserted(&mut self, _node: Node) {
        self.inserted += 1;
    }

    #[inline]
    fn deleted(&mut self, _node: Node) {
        self.deleted += 1;
    }

    #[inline]
    fn decreased(&mut self, _node: Node) {
        self.decreased += 1;
    }
}

/// Keeps every node that came off the queue, in the order they came off.
///
/// [`RankTargets`] exists because a search over a whole continent settles
/// eighteen million nodes and keeping all of them is a vector as long as the
/// graph. A search over the cells of a partition settles a few thousand, so
/// for that one the whole order costs a few tens of kilobytes and is worth
/// having: it is what lets a check say which level each settled node was
/// stepped over at, and that is a property no test of the distances would ever
/// notice going wrong.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettledNodes<Node = NodeID> {
    settled: Vec<Node>,
}

/// Written out rather than derived, as a derived one would ask the node type
/// to have a default of its own and nothing here ever needs one.
impl<Node> Default for SettledNodes<Node> {
    fn default() -> Self {
        Self {
            settled: Vec::new(),
        }
    }
}

impl<Node> SettledNodes<Node> {
    /// The nodes that came off the queue, in the order they came off.
    #[must_use]
    pub fn settled(&self) -> &[Node] {
        &self.settled
    }
}

impl<Node> HeapStats<Node> for SettledNodes<Node> {
    #[inline]
    fn inserted(&mut self, _node: Node) {}

    #[inline]
    fn deleted(&mut self, node: Node) {
        self.settled.push(node);
    }

    #[inline]
    fn decreased(&mut self, _node: Node) {}
}

/// Every node that went onto the queue, and every node that came off it.
///
/// [`SettledNodes`] keeps what a search decided. This also keeps what it looked
/// at and never got round to deciding, which is the other half of a picture of
/// a search: a node it settled is one it knows the distance to, and a node it
/// only reached is one it had a distance for and stopped before using. Drawn
/// alike the two say the search did more work than it did.
///
/// Both are kept in the order the queue handed them over, and neither is
/// de-duplicated because neither has to be: a node goes onto an addressable
/// queue once and comes off it once. So the reached hold every node the search
/// touched, exactly once each, and the settled are a subset of them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frontier<Node = NodeID> {
    settled: Vec<Node>,
    reached: Vec<Node>,
}

/// Written out rather than derived, as a derived one would ask the node type
/// to have a default of its own and nothing here ever needs one.
impl<Node> Default for Frontier<Node> {
    fn default() -> Self {
        Self {
            settled: Vec::new(),
            reached: Vec::new(),
        }
    }
}

impl<Node> Frontier<Node> {
    /// The nodes that came off the queue, in the order they came off.
    #[must_use]
    pub fn settled(&self) -> &[Node] {
        &self.settled
    }

    /// Every node that went onto the queue, in the order it went on.
    ///
    /// This holds the settled as well. What was reached and not settled is the
    /// one without the other, which is a question for whoever is asking rather
    /// than a second list to keep.
    #[must_use]
    pub fn reached(&self) -> &[Node] {
        &self.reached
    }
}

impl<Node> HeapStats<Node> for Frontier<Node> {
    #[inline]
    fn inserted(&mut self, node: Node) {
        self.reached.push(node);
    }

    #[inline]
    fn deleted(&mut self, node: Node) {
        self.settled.push(node);
    }

    #[inline]
    fn decreased(&mut self, _node: Node) {}
}

/// The node settled at each power of two, and nothing else.
///
/// The place of a node in the settling is its Dijkstra rank from the source,
/// so one walk of a graph hands back a target for every rank at once rather
/// than one target per search. Only the powers of two are worth keeping: a
/// rank plot is drawn on a log axis and every rank between two of them lands
/// in the same bucket.
///
/// Keeping the whole order instead would be a vector as long as the graph.
/// On a continental network that is eighteen million nodes, and one of those
/// per thread, against the two dozen entries this holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RankTargets<Node = NodeID> {
    settled: usize,
    /// the rank and the node settled at it, in increasing order of rank
    targets: Vec<(usize, Node)>,
}

/// Written out rather than derived, as a derived one would ask the node type
/// to have a default of its own and nothing here ever needs one.
impl<Node> Default for RankTargets<Node> {
    fn default() -> Self {
        Self {
            settled: 0,
            targets: Vec::new(),
        }
    }
}

impl<Node> RankTargets<Node> {
    /// The nodes settled at a power of two, each with the rank it sits at.
    #[must_use]
    pub fn targets(&self) -> &[(usize, Node)] {
        &self.targets
    }

    /// How many nodes were settled altogether, which is the rank the walk ran
    /// out at.
    #[must_use]
    pub fn settled_count(&self) -> usize {
        self.settled
    }
}

impl<Node> HeapStats<Node> for RankTargets<Node> {
    #[inline]
    fn inserted(&mut self, _node: Node) {}

    #[inline]
    fn deleted(&mut self, node: Node) {
        self.settled += 1;
        // two instructions per settled node, against a push of every one of
        // them, which is what this exists instead of
        if self.settled.is_power_of_two() {
            self.targets.push((self.settled, node));
        }
    }

    #[inline]
    fn decreased(&mut self, _node: Node) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collecting_nothing_takes_up_nothing() {
        assert_eq!(std::mem::size_of::<Untracked>(), 0);
    }

    #[test]
    fn the_counters_count_what_they_are_told() {
        let mut counters = Counters::default();
        counters.deleted(1);
        counters.deleted(2);
        counters.inserted(3);
        counters.decreased(4);
        counters.decreased(5);
        counters.decreased(6);

        assert_eq!(
            counters,
            Counters {
                inserted: 1,
                deleted: 2,
                decreased: 3,
            }
        );
    }

    #[test]
    fn only_the_powers_of_two_are_kept() {
        let mut ranks = RankTargets::default();
        for node in 0..5 {
            ranks.deleted(node);
        }

        // the first node settled is rank one, and 1, 2 and 4 are within a walk
        // of five while 8 is not
        assert_eq!(ranks.targets(), &[(1, 0), (2, 1), (4, 3)]);
        assert_eq!(ranks.settled_count(), 5);
    }

    #[test]
    fn the_frontier_keeps_what_went_on_and_what_came_off() {
        let mut frontier = Frontier::default();
        frontier.inserted(1);
        frontier.inserted(2);
        frontier.inserted(3);
        frontier.decreased(2);
        frontier.deleted(1);
        frontier.deleted(2);

        // the reached hold the settled as well, and a decrease is neither
        assert_eq!(frontier.reached(), &[1, 2, 3]);
        assert_eq!(frontier.settled(), &[1, 2]);
    }

    #[test]
    fn a_walk_that_settled_nothing_has_no_ranks() {
        let ranks = RankTargets::<NodeID>::default();
        assert!(ranks.targets().is_empty());
        assert_eq!(ranks.settled_count(), 0);
    }

    /// What else happens on the queue is not worth a branch here, as the
    /// ranks are counted in deletions and nothing else.
    #[test]
    fn the_ranks_count_only_what_was_deleted() {
        let mut ranks = RankTargets::default();
        ranks.deleted(7);
        ranks.inserted(8);
        ranks.decreased(9);

        assert_eq!(ranks.targets(), &[(1, 7)]);
        assert_eq!(ranks.settled_count(), 1);
    }
}
