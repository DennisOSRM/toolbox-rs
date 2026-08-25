/// Implementation of a one-to-many Dijkstra that uses the adresseable heap
/// as its priority queue.
///
/// The main advantage of this implementation is that it stores the entire
/// search space of each run in its internal structures. From there paths can
/// be unpacked.
use crate::{
    dense_heap::DenseHeap,
    graph::{Arcs, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
};

use log::debug;

/// A search from one node to a set of them, counting nothing.
///
/// This is the plain machine, and what a run whose time is being taken wants:
/// no counters, no targets kept, nothing carried that a measurement would be
/// measuring instead of the search.
pub type OneToManyDijkstra = OneToManySearch<Untracked>;

/// The same search, counting what its queue did.
pub type TrackedOneToManyDijkstra = OneToManySearch<Counters>;

pub struct OneToManySearch<S: HeapStats<NodeID>> {
    /// A queue that finds a node in an array rather than in a map.
    ///
    /// The graph this search runs over is a cell, whose nodes are numbered
    /// from nothing with no gaps, so the array is as long as the cell is wide
    /// and the search never asks a hash anything. It was a map before, and a
    /// relaxation asked it three or four separate questions -- is this node
    /// on the queue, is it still on it, what is it held at, now lower it --
    /// where the array answers all of them in one look. On the coarse levels
    /// of a continent that inner loop runs some thousand million times.
    queue: DenseHeap<S>,
    reached_target_count: usize,
}

impl<S: HeapStats<NodeID>> Default for OneToManySearch<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> OneToManySearch<S> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: DenseHeap::new(),
            reached_target_count: 0,
        }
    }

    /// What the last run did, as far as the collector was asked to keep. The
    /// queue is what counts it, as everything worth counting is something the
    /// queue was asked to do.
    pub fn stats(&self) -> &S {
        self.queue.stats()
    }

    /// clears the search space stored in the queue.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.reached_target_count = 0;
    }

    /// retrieves the number of nodes that were explored (not settled) during
    /// a search.
    pub fn search_space_len(&self) -> usize {
        self.queue.inserted_len()
    }

    /// return the last known distance of a node from a queue
    pub fn distance(&self, node: NodeID) -> usize {
        self.queue.weight(node)
    }

    /// run a path computation from s to t on some graph. The object is reusable
    /// to run consecutive searches, even on different graphs. It is cleared on
    /// every run, which saves on allocations.
    pub fn run<G: Arcs<u32>>(&mut self, graph: &G, source: NodeID, targets: &[NodeID]) -> bool {
        let wanted =
            rustc_hash::FxHashMap::<NodeID, ()>::from_iter(targets.iter().map(|&x| (x, ())));
        self.walk(graph, source, wanted.len(), |node| {
            wanted.contains_key(&node)
        })
    }

    /// The same search, to the first `count` nodes of the graph.
    ///
    /// A search whose targets are a prefix of the numbering does not need to
    /// be told which nodes they are. [`run`](Self::run) builds a set of them
    /// and asks it once per settled node, which for a customization is a set
    /// the size of the answer being computed: one built and thrown away per
    /// search, and one lookup per settle, to ask a question that is
    /// `node < count`. The border nodes of a cell are numbered first exactly
    /// so that it is.
    pub fn run_to_leading<G: Arcs<u32>>(
        &mut self,
        graph: &G,
        source: NodeID,
        count: usize,
    ) -> bool {
        self.walk(graph, source, count, |node| node < count)
    }

    /// What both of them do, differing only in how a target is recognised.
    fn walk<G: Arcs<u32>>(
        &mut self,
        graph: &G,
        source: NodeID,
        wanted: usize,
        is_target: impl Fn(NodeID) -> bool,
    ) -> bool {
        // clear the search space
        self.clear();

        debug!("[start] source: {source:?}, {wanted} targets");

        // prime queue
        self.queue.insert(source, 0, source);
        debug!("[push] {source} at distance {}", self.queue.weight(source));

        // iteratively search the graph
        while !self.queue.is_empty() && self.reached_target_count < wanted {
            // settle next node from queue
            let u = self.queue.delete_min();
            let distance = self.queue.weight(u);

            debug!("[pop] {u} at distance {distance}");

            // check if target is reached
            if is_target(u) {
                self.reached_target_count += 1;
                debug!("[done] reached {u} at {distance}");
            }

            // relax outgoing edges, each in one look at the queue: whether
            // the node is on it, what it is held at and whether this is an
            // improvement are the same question asked of the same slot
            for edge in graph.edge_range(u) {
                let v = graph.target(edge);
                let new_distance = distance + graph.weight(edge) as usize;
                self.queue.insert_or_decrease(v, new_distance, u);
            }
        }

        self.reached_target_count == wanted
    }

    /// retrieve path from the node to the queue according to the search space
    /// stored in the priority queue. It's stored in reverse node order (from
    /// target to source) and thus reversed before returning.
    pub fn retrieve_node_path(&self, target: NodeID) -> Option<Vec<NodeID>> {
        if !self.queue.inserted(target) {
            // if no path was found or target was not reached, return None
            return None;
        }

        let mut path = vec![target];
        let mut node = target;
        loop {
            // since the target was inserted (as checked above) and the sources
            // parent is the source node of the search itself, this loop will
            // terminate.
            let parent = self.queue.data(node);
            if parent == node {
                // reverse order to go from source to target
                path.reverse();
                return Some(path);
            }
            path.push(parent);
            node = parent;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        edge::InputEdge,
        graph::Graph,
        graph::NodeID,
        one_to_many_dijkstra::{OneToManyDijkstra, TrackedOneToManyDijkstra},
        static_graph::StaticGraph,
    };

    /// A node reached again by a shorter way keeps the node that way came
    /// from, or the walk back stops in the middle of the path.
    #[test]
    fn a_node_reached_again_keeps_the_way_it_was_reached_by() {
        let edges = vec![
            InputEdge::new(0, 1, 10_u32),
            InputEdge::new(0, 2, 1_u32),
            InputEdge::new(2, 1, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = OneToManyDijkstra::new();

        assert!(dijkstra.run(&graph, 0, &[1]));
        assert_eq!(dijkstra.distance(1), 2);
        assert_eq!(dijkstra.retrieve_node_path(1), Some(vec![0, 2, 1]));
    }

    /// The one-to-many search counts the same three things, and stops once it
    /// has settled every target rather than walking the rest of the graph.
    #[test]
    fn a_search_that_stops_early_counts_only_what_it_did() {
        let edges = vec![
            InputEdge::new(0, 1, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 3, 1_u32),
            InputEdge::new(3, 4, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = TrackedOneToManyDijkstra::new();

        assert!(dijkstra.run(&graph, 0, &[2]));
        // it settled 0, 1 and 2 and stopped there, so node 4 was never reached
        assert_eq!(dijkstra.stats().deleted, 3);
        assert!(
            dijkstra.stats().inserted < 5,
            "the whole line was walked: {:?}",
            dijkstra.stats()
        );
    }

    fn create_graph() -> StaticGraph<u32> {
        let edges = vec![
            InputEdge::new(0, 1, 3_u32),
            InputEdge::new(1, 2, 3_u32),
            InputEdge::new(4, 2, 1_u32),
            InputEdge::new(2, 3, 6_u32),
            InputEdge::new(0, 4, 2_u32),
            InputEdge::new(4, 5, 2_u32),
            InputEdge::new(5, 3, 7_u32),
            InputEdge::new(1, 5, 2_u32),
        ];
        let graph = StaticGraph::<u32>::new(edges);
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());

        graph
    }

    #[test]
    fn simple_graph() {
        let graph = create_graph();

        let mut dijkstra = OneToManyDijkstra::new();
        let success = dijkstra.run(&graph, 0, &[3]);
        assert!(success);
        assert_eq!(6, dijkstra.search_space_len());
        assert_eq!(9, dijkstra.distance(3));
    }

    #[test]
    fn apsp() {
        let graph = create_graph();

        let no = usize::MAX;

        let results_table = [
            [0, 3, 3, 9, 2, 4],
            [no, 0, 3, 9, no, 2],
            [no, no, 0, 6, no, no],
            [no, no, no, 0, no, no],
            [no, no, 1, 7, 0, 2],
            [no, no, no, 7, no, 0],
        ];

        let mut dijkstra = OneToManyDijkstra::new();
        for (i, &table) in results_table.iter().enumerate() {
            let success = dijkstra.run(&graph, i, &[0, 1, 2, 3, 4, 5]);
            assert_eq!(success, !results_table[i].iter().any(|x| { *x == no })); // find any
            for (j, result) in table.iter().enumerate() {
                assert_eq!(*result, dijkstra.distance(j));
            }
        }
    }

    #[test]
    fn retrieve_node_path() {
        let graph = create_graph();
        let mut dijkstra = OneToManyDijkstra::default();
        let success = dijkstra.run(&graph, 0, &[3]);
        assert!(success);
        assert_eq!(9, dijkstra.distance(3));
        let computed_path = dijkstra.retrieve_node_path(3).unwrap();
        let expected_path = vec![0, 4, 2, 3];

        assert_eq!(computed_path, expected_path);
    }

    #[test]
    fn decrease_key_in_search() {
        let edges = vec![
            InputEdge::new(0, 1, 7_u32),
            InputEdge::new(0, 2, 3_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(1, 3, 6_u32),
            InputEdge::new(2, 4, 8_u32),
            InputEdge::new(3, 5, 2_u32),
            InputEdge::new(4, 3, 2_u32),
            InputEdge::new(4, 5, 8_u32),
        ];
        let graph = StaticGraph::new(edges);

        let mut dijkstra = OneToManyDijkstra::new();
        let success = dijkstra.run(&graph, 0, &[5]);
        assert!(success);
        assert_eq!(dijkstra.distance(5), 15);
    }

    #[test]
    fn larger_graph() {
        // regression test from handling DIMACS data set
        let edges = vec![
            InputEdge::new(3, 12, 2852_u32),
            InputEdge::new(3, 13, 1641_u32),
            InputEdge::new(3, 26, 1334_u32),
            InputEdge::new(3, 14, 425_u32),
            InputEdge::new(3, 27, 1380_u32),
            InputEdge::new(28, 29, 2713_u32),
            InputEdge::new(28, 30, 2378_u32),
            InputEdge::new(28, 31, 1114_u32),
            InputEdge::new(28, 8, 1013_u32),
            InputEdge::new(32, 30, 1225_u32),
            InputEdge::new(32, 33, 892_u32),
            InputEdge::new(32, 31, 2375_u32),
            InputEdge::new(34, 33, 2497_u32),
            InputEdge::new(34, 35, 885_u32),
            InputEdge::new(34, 31, 1332_u32),
            InputEdge::new(36, 37, 2886_u32),
            InputEdge::new(36, 38, 864_u32),
            InputEdge::new(36, 39, 126_u32),
            InputEdge::new(37, 36, 2886_u32),
            InputEdge::new(38, 36, 864_u32),
            InputEdge::new(38, 40, 3560_u32),
            InputEdge::new(38, 41, 1770_u32),
            InputEdge::new(38, 42, 826_u32),
            InputEdge::new(40, 38, 3560_u32),
            InputEdge::new(40, 39, 3335_u32),
            InputEdge::new(40, 43, 2295_u32),
            InputEdge::new(41, 38, 1770_u32),
            InputEdge::new(1, 15, 667_u32),
            InputEdge::new(1, 44, 901_u32),
            InputEdge::new(1, 9, 1233_u32),
            InputEdge::new(44, 1, 901_u32),
            InputEdge::new(45, 46, 1638_u32),
            InputEdge::new(45, 47, 889_u32),
            InputEdge::new(45, 48, 2582_u32),
            InputEdge::new(46, 45, 1638_u32),
            InputEdge::new(47, 45, 889_u32),
            InputEdge::new(47, 49, 1311_u32),
            InputEdge::new(47, 11, 508_u32),
            InputEdge::new(49, 47, 1311_u32),
            InputEdge::new(11, 47, 508_u32),
            InputEdge::new(11, 7, 3106_u32),
            InputEdge::new(11, 50, 1979_u32),
            InputEdge::new(11, 16, 1334_u32),
            InputEdge::new(4, 26, 1917_u32),
            InputEdge::new(4, 51, 859_u32),
            InputEdge::new(4, 17, 1140_u32),
            InputEdge::new(4, 2, 2888_u32),
            InputEdge::new(4, 52, 1885_u32),
            InputEdge::new(26, 3, 1334_u32),
            InputEdge::new(26, 4, 1917_u32),
            InputEdge::new(26, 51, 1657_u32),
            InputEdge::new(51, 4, 859_u32),
            InputEdge::new(51, 26, 1657_u32),
            InputEdge::new(51, 53, 1253_u32),
            InputEdge::new(51, 54, 2474_u32),
            InputEdge::new(27, 3, 1380_u32),
            InputEdge::new(27, 53, 690_u32),
            InputEdge::new(27, 8, 3284_u32),
            InputEdge::new(2, 18, 1249_u32),
            InputEdge::new(2, 4, 2888_u32),
            InputEdge::new(2, 55, 1560_u32),
            InputEdge::new(52, 4, 1885_u32),
            InputEdge::new(52, 55, 1525_u32),
            InputEdge::new(52, 56, 2467_u32),
            InputEdge::new(53, 51, 1253_u32),
            InputEdge::new(53, 27, 690_u32),
            InputEdge::new(53, 29, 552_u32),
            InputEdge::new(29, 28, 2713_u32),
            InputEdge::new(29, 53, 552_u32),
            InputEdge::new(29, 57, 1196_u32),
            InputEdge::new(0, 19, 2224_u32),
            InputEdge::new(0, 5, 584_u32),
            InputEdge::new(0, 58, 2113_u32),
            InputEdge::new(0, 59, 1065_u32),
            InputEdge::new(5, 20, 491_u32),
            InputEdge::new(5, 0, 584_u32),
            InputEdge::new(5, 60, 904_u32),
            InputEdge::new(60, 5, 904_u32),
            InputEdge::new(60, 30, 1111_u32),
            InputEdge::new(60, 8, 2549_u32),
            InputEdge::new(58, 0, 2113_u32),
            InputEdge::new(58, 30, 491_u32),
            InputEdge::new(58, 61, 2112_u32),
            InputEdge::new(59, 0, 1065_u32),
            InputEdge::new(59, 62, 983_u32),
            InputEdge::new(59, 63, 4556_u32),
            InputEdge::new(30, 28, 2378_u32),
            InputEdge::new(30, 32, 1225_u32),
            InputEdge::new(30, 60, 1111_u32),
            InputEdge::new(30, 58, 491_u32),
            InputEdge::new(61, 58, 2112_u32),
            InputEdge::new(61, 33, 573_u32),
            InputEdge::new(61, 63, 1038_u32),
            InputEdge::new(61, 64, 3897_u32),
            InputEdge::new(33, 32, 892_u32),
            InputEdge::new(33, 34, 2497_u32),
            InputEdge::new(33, 61, 573_u32),
            InputEdge::new(62, 59, 983_u32),
            InputEdge::new(62, 39, 1070_u32),
            InputEdge::new(62, 65, 5245_u32),
            InputEdge::new(63, 59, 4556_u32),
            InputEdge::new(63, 61, 1038_u32),
            InputEdge::new(63, 65, 1544_u32),
            InputEdge::new(63, 66, 3563_u32),
            InputEdge::new(39, 36, 126_u32),
            InputEdge::new(39, 40, 3335_u32),
            InputEdge::new(39, 62, 1070_u32),
            InputEdge::new(42, 38, 826_u32),
            InputEdge::new(42, 67, 672_u32),
            InputEdge::new(42, 6, 989_u32),
            InputEdge::new(67, 42, 672_u32),
            InputEdge::new(6, 42, 989_u32),
            InputEdge::new(6, 21, 424_u32),
            InputEdge::new(55, 2, 1560_u32),
            InputEdge::new(55, 52, 1525_u32),
            InputEdge::new(55, 68, 2967_u32),
            InputEdge::new(56, 52, 2467_u32),
            InputEdge::new(56, 35, 414_u32),
            InputEdge::new(56, 54, 1016_u32),
            InputEdge::new(35, 34, 885_u32),
            InputEdge::new(35, 56, 414_u32),
            InputEdge::new(35, 68, 1242_u32),
            InputEdge::new(48, 45, 2582_u32),
            InputEdge::new(48, 69, 828_u32),
            InputEdge::new(48, 64, 1589_u32),
            InputEdge::new(48, 70, 1657_u32),
            InputEdge::new(69, 48, 828_u32),
            InputEdge::new(69, 7, 371_u32),
            InputEdge::new(69, 71, 861_u32),
            InputEdge::new(7, 11, 3106_u32),
            InputEdge::new(7, 69, 371_u32),
            InputEdge::new(7, 22, 742_u32),
            InputEdge::new(65, 62, 5245_u32),
            InputEdge::new(65, 63, 1544_u32),
            InputEdge::new(65, 43, 1306_u32),
            InputEdge::new(66, 63, 3563_u32),
            InputEdge::new(66, 64, 1202_u32),
            InputEdge::new(66, 10, 997_u32),
            InputEdge::new(43, 40, 2295_u32),
            InputEdge::new(43, 65, 1306_u32),
            InputEdge::new(64, 61, 3897_u32),
            InputEdge::new(64, 48, 1589_u32),
            InputEdge::new(64, 66, 1202_u32),
            InputEdge::new(64, 70, 1667_u32),
            InputEdge::new(10, 66, 997_u32),
            InputEdge::new(10, 72, 616_u32),
            InputEdge::new(10, 23, 1463_u32),
            InputEdge::new(57, 29, 1196_u32),
            InputEdge::new(57, 31, 1970_u32),
            InputEdge::new(57, 54, 508_u32),
            InputEdge::new(31, 28, 1114_u32),
            InputEdge::new(31, 32, 2375_u32),
            InputEdge::new(31, 34, 1332_u32),
            InputEdge::new(31, 57, 1970_u32),
            InputEdge::new(54, 51, 2474_u32),
            InputEdge::new(54, 56, 1016_u32),
            InputEdge::new(54, 57, 508_u32),
            InputEdge::new(8, 28, 1013_u32),
            InputEdge::new(8, 27, 3284_u32),
            InputEdge::new(8, 60, 2549_u32),
            InputEdge::new(8, 24, 1003_u32),
            InputEdge::new(9, 1, 1233_u32),
            InputEdge::new(9, 25, 1229_u32),
            InputEdge::new(9, 70, 7863_u32),
            InputEdge::new(68, 55, 2967_u32),
            InputEdge::new(68, 35, 1242_u32),
            InputEdge::new(68, 70, 2667_u32),
            InputEdge::new(70, 48, 1657_u32),
            InputEdge::new(70, 64, 1667_u32),
            InputEdge::new(70, 9, 7863_u32),
            InputEdge::new(70, 68, 2667_u32),
            InputEdge::new(71, 69, 861_u32),
            InputEdge::new(72, 10, 616_u32),
            InputEdge::new(50, 11, 1979_u32),
        ];
        let graph = StaticGraph::new(edges);

        let mut dijkstra = OneToManyDijkstra::new();
        let success = dijkstra.run(&graph, 1, &[19]);
        assert!(success);
        assert_eq!(dijkstra.distance(19), 21109);
    }

    /// What a Dijkstra over a standard library heap makes of a graph, which
    /// is the answer both searches of this crate have to give as well.
    ///
    /// Checking the two of them against each other is not enough: they share
    /// a heap, so a fault in it moves both the same way and they go on
    /// agreeing. This reference shares nothing with either.
    fn distances_from<G: crate::graph::Arcs<u32>>(graph: &G, source: NodeID) -> Vec<usize> {
        use std::{cmp::Reverse, collections::BinaryHeap};

        let mut settled = vec![usize::MAX; graph.number_of_nodes()];
        let mut queue = BinaryHeap::new();
        queue.push(Reverse((0_usize, source)));
        while let Some(Reverse((cost, node))) = queue.pop() {
            if settled[node] != usize::MAX {
                continue;
            }
            settled[node] = cost;
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                if settled[target] == usize::MAX {
                    queue.push(Reverse((cost + graph.weight(edge) as usize, target)));
                }
            }
        }
        settled
    }

    /// Both searches of the crate, over graphs drawn without a pattern, held
    /// against a search that has nothing in common with them.
    ///
    /// The graphs are drawn so that a search has to choose between two ways to
    /// a node rather than walk one path, which is what puts the ordering of the
    /// heap to work. That the ordering is really what this covers is easiest
    /// seen by breaking it: with the weight of a lowered key left stale in the
    /// heap, this test fails and every older search test still passes.
    #[test]
    fn both_searches_agree_with_a_search_that_shares_nothing_with_them() {
        use crate::unidirectional_dijkstra::UnidirectionalDijkstra;
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x0217);
        let mut shortcuts = 0;
        for round in 0..20 {
            let nodes = 12 + round;
            // a path through every node, so nothing is out of reach, and then
            // arcs thrown in at random, which is what gives a search the
            // chance to reach a node once and then reach it again cheaper
            let mut edges = Vec::new();
            for node in 0..nodes - 1 {
                edges.push(InputEdge::new(node, node + 1, 1 + rng.random_range(0..9)));
            }
            for _ in 0..nodes * 2 {
                let source = rng.random_range(0..nodes);
                let target = rng.random_range(0..nodes);
                if source != target {
                    edges.push(InputEdge::new(source, target, 1 + rng.random_range(0..20)));
                }
            }
            let graph = StaticGraph::<u32>::new(edges);

            let targets = (0..nodes).collect::<Vec<_>>();
            let mut one_to_many = OneToManyDijkstra::new();
            let mut one_to_one = UnidirectionalDijkstra::new();
            for source in 0..nodes {
                let expected = distances_from(&graph, source);
                one_to_many.run(&graph, source, &targets);
                for (target, &cost) in expected.iter().enumerate() {
                    assert_eq!(
                        one_to_many.distance(target),
                        cost,
                        "one to many, round {round}: from {source} to {target}"
                    );
                    assert_eq!(
                        one_to_one.run(&graph, source, target as NodeID),
                        cost,
                        "one to one, round {round}: from {source} to {target}"
                    );
                    // a target that came out cheaper than walking the path
                    // through every node says the arcs thrown in are worth
                    // taking, i.e. that a search over this graph has more than
                    // one way to reach a node
                    if target > source && cost < target - source {
                        shortcuts += 1;
                    }
                }
            }
        }
        assert!(
            shortcuts > 0,
            "every graph in this test was walked best along its path, so none \
             of them made a search choose between two ways to a node"
        );
    }
}
