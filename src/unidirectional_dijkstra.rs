/// Implementation of a unidirectional Dijkstra that uses the dense heap as its
/// priority queue.
///
/// The main advantage of this implementation is that it stores the entire
/// search space of each run in its internal structures. From there paths can
/// be unpacked.
///
/// The queue finds a node by indexing an array with it rather than by looking
/// it up in a map. That matters for what this is mostly used for here, which
/// is to say what a search over the cells of a partition is worth: the two
/// have to answer the same question with the same machinery underneath, or the
/// ratio between them is partly a ratio between two ways of finding a node.
use crate::{
    dense_heap::DenseHeap,
    graph::{Graph, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
};

use log::debug;

/// A search from one node to another, counting nothing.
///
/// This is the plain machine, and what a run whose time is being taken wants:
/// no counters, no targets kept, nothing carried that a measurement would be
/// measuring instead of the search.
pub type UnidirectionalDijkstra = UnidirectionalSearch<Untracked>;

/// The same search, counting what its queue did.
pub type TrackedUnidirectionalDijkstra = UnidirectionalSearch<Counters>;

pub struct UnidirectionalSearch<S: HeapStats<NodeID>> {
    queue: DenseHeap<S>,
    upper_bound: usize,
}

impl<S: HeapStats<NodeID>> Default for UnidirectionalSearch<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> UnidirectionalSearch<S> {
    #[must_use]
    pub fn new() -> Self {
        let queue = DenseHeap::<S>::new();
        Self {
            queue,
            upper_bound: usize::MAX,
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
        self.upper_bound = usize::MAX;
    }

    /// What it cost to reach a node, and `usize::MAX` for one the last run
    /// never reached.
    ///
    /// A run that stopped at its target has this for every node it settled on
    /// the way, and one that ran until the queue was empty has it for
    /// everything the source can reach.
    #[must_use]
    pub fn distance(&self, node: NodeID) -> usize {
        self.queue.weight(node)
    }

    /// retrieves the number of nodes that were explored (not settled) during
    /// a search.
    pub fn search_space_len(&self) -> usize {
        self.queue.inserted_len()
    }

    /// run a path computation from s to t on some graph. The object is reusable
    /// to run consecutive searches, even on different graphs. It is cleared on
    /// every run, which saves on allocations.
    pub fn run<G: Graph<u32>>(&mut self, graph: &G, s: NodeID, t: NodeID) -> usize {
        // clear the search space
        self.clear();

        debug!("[start] source: {s}, target: {t}");

        // prime queue
        self.queue.insert(s, 0, s);
        debug!("[push] {s} at distance {}", self.queue.weight(s));

        // iteratively search the graph
        while !self.queue.is_empty() && self.upper_bound == usize::MAX {
            // settle next node from queue
            let u = self.queue.delete_min();
            let distance = self.queue.weight(u);

            debug!("[pop] {u} at distance {distance}");

            // check if target is reached
            if u == t {
                self.upper_bound = distance;
                debug!("[done] reached {t} at {distance}");
                return self.upper_bound;
            }

            // relax outgoing edges
            for edge in graph.edge_range(u) {
                debug!("[relax] edge {edge}");
                let v = graph.target(edge);
                let new_distance = distance + *graph.data(edge) as usize;

                self.queue.insert_or_decrease(v, new_distance, u);
            }
        }

        self.upper_bound
    }

    /// retrieve path from the node to the queue according to the search space
    /// stored in the priority queue. It's stored in reverse node order (from
    /// target to source) and thus reversed before returning.
    pub fn retrieve_node_path(&self, target: NodeID) -> Option<Vec<NodeID>> {
        if self.upper_bound == usize::MAX || !self.queue.inserted(target) {
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
        heap_stats::{Counters, RankTargets, Untracked},
        static_graph::StaticGraph,
        unidirectional_dijkstra::{
            TrackedUnidirectionalDijkstra, UnidirectionalDijkstra, UnidirectionalSearch,
        },
    };

    /// The parent of a node that is reached again by a shorter way.
    ///
    /// A node is inserted with the node it was reached from as its parent, and
    /// a later, shorter way to it has to hand it the node that shorter way came
    /// from. Handing it itself instead leaves it looking like the node the
    /// search started at, which is where the walk back stops: the path then
    /// begins in the middle of itself and says nothing about how it got there.
    #[test]
    fn a_node_reached_again_keeps_the_way_it_was_reached_by() {
        // 0 to 1 costs ten the direct way and two through node 2, so node 1 is
        // reached once and then reached again by something better
        let edges = vec![
            InputEdge::new(0, 1, 10_u32),
            InputEdge::new(0, 2, 1_u32),
            InputEdge::new(2, 1, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = UnidirectionalDijkstra::new();

        assert_eq!(dijkstra.run(&graph, 0, 1), 2);
        assert_eq!(dijkstra.retrieve_node_path(1), Some(vec![0, 2, 1]));
    }

    /// Whatever path comes back has to be a path of the graph, and it has to
    /// cost what the search said it costs. That holds the two together on
    /// graphs nobody worked out by hand.
    #[test]
    fn a_retrieved_path_walks_the_graph_and_costs_what_was_reported() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_D1_5A);
        for round in 0..20 {
            let count = 8 + round;
            let mut edges = Vec::new();
            for source in 0..count {
                for target in 0..count {
                    if source != target && rng.random_range(0..3) == 0 {
                        edges.push(InputEdge::new(source, target, rng.random_range(1..20_u32)));
                    }
                }
            }
            if edges.is_empty() {
                continue;
            }
            let graph = StaticGraph::new(edges);
            let mut dijkstra = UnidirectionalDijkstra::new();

            for source in 0..count {
                for target in 0..count {
                    let distance = dijkstra.run(&graph, source, target);
                    if distance == usize::MAX {
                        continue;
                    }
                    let path = dijkstra
                        .retrieve_node_path(target)
                        .expect("a path was found but not handed back");
                    assert_eq!(path.first(), Some(&source), "round {round}: {path:?}");
                    assert_eq!(path.last(), Some(&target), "round {round}: {path:?}");

                    let mut walked = 0_usize;
                    for step in path.windows(2) {
                        let edge = graph
                            .find_edge(step[0], step[1])
                            .expect("the path takes an arc the graph does not have");
                        walked += *graph.data(edge) as usize;
                    }
                    assert_eq!(walked, distance, "round {round}: {path:?}");
                }
            }
        }
    }

    /// A search that collects nothing is the search that was there before any
    /// of this: no counters, no list of nodes, nothing to carry.
    #[test]
    fn collecting_nothing_costs_nothing() {
        use std::mem::size_of;
        assert_eq!(
            size_of::<UnidirectionalSearch<Untracked>>(),
            size_of::<UnidirectionalDijkstra>(),
        );
        assert!(
            size_of::<TrackedUnidirectionalDijkstra>() > size_of::<UnidirectionalDijkstra>(),
            "counting is supposed to take up room, or it is not counting"
        );
    }

    /// What the three numbers mean, on a graph small enough to work them out
    /// by hand.
    ///
    /// A path of four nodes in a row: the search settles all four, puts three
    /// of them on the queue, and never finds a shorter way to any of them, as
    /// there is only one way to each.
    #[test]
    fn a_line_is_settled_once_and_never_improved() {
        let edges = vec![
            InputEdge::new(0, 1, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 3, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = TrackedUnidirectionalDijkstra::new();

        assert_eq!(dijkstra.run(&graph, 0, 3), 3);
        assert_eq!(
            *dijkstra.stats(),
            Counters {
                // the source goes onto the queue too, which is what the
                // search itself used to count and forget
                inserted: 4,
                deleted: 4,
                decreased: 0,
            }
        );
    }

    /// The queue counts what the queue did, so what it says it inserted is
    /// what it holds. Counting from the search instead meant counting at the
    /// places the search remembered to count at, and the node it primes its
    /// queue with sits outside the loop it relaxes in: the two numbers were
    /// out by exactly that one.
    #[test]
    fn what_was_inserted_is_what_the_queue_holds() {
        let graph = create_graph();
        let mut dijkstra = TrackedUnidirectionalDijkstra::new();

        dijkstra.run(&graph, 0, 3);
        assert_eq!(dijkstra.stats().inserted, dijkstra.search_space_len());
    }

    /// And a graph that does offer a shorter way to a node already reached
    /// counts it, which is the number the other two say nothing about.
    #[test]
    fn a_node_reached_twice_is_counted_as_improved() {
        let edges = vec![
            InputEdge::new(0, 1, 10_u32),
            InputEdge::new(0, 2, 1_u32),
            InputEdge::new(2, 1, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = TrackedUnidirectionalDijkstra::new();

        assert_eq!(dijkstra.run(&graph, 0, 1), 2);
        assert_eq!(dijkstra.stats().decreased, 1);
    }

    /// The rank of a node is where it was settled, and one walk of the graph
    /// hands back a target for every rank rather than one target per search.
    #[test]
    fn one_walk_hands_back_a_target_for_every_rank() {
        let edges = vec![
            InputEdge::new(0, 1, 1_u32),
            InputEdge::new(1, 2, 1_u32),
            InputEdge::new(2, 3, 1_u32),
        ];
        let graph = StaticGraph::new(edges);
        let mut dijkstra = UnidirectionalSearch::<RankTargets>::new();

        // no node of this graph is node 9, so the search runs out rather than
        // stopping early, which is how the sampler walks the whole of it
        dijkstra.run(&graph, 0, 9);
        assert_eq!(dijkstra.stats().settled_count(), 4);
        assert_eq!(dijkstra.stats().targets(), &[(1, 0), (2, 1), (4, 3)]);
    }

    /// Each run says what that run did and nothing about the one before it.
    #[test]
    fn a_second_run_does_not_carry_the_first() {
        let graph = create_graph();
        let mut dijkstra = TrackedUnidirectionalDijkstra::new();

        dijkstra.run(&graph, 0, 3);
        let first = *dijkstra.stats();
        dijkstra.run(&graph, 0, 3);

        assert_eq!(*dijkstra.stats(), first);
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

        let mut dijkstra = UnidirectionalDijkstra::default();
        let distance = dijkstra.run(&graph, 0, 3);
        assert_eq!(6, dijkstra.search_space_len());
        assert_eq!(9, distance);
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

        let mut dijkstra = UnidirectionalDijkstra::new();
        for (i, &table) in results_table.iter().enumerate() {
            for (j, result) in table.iter().enumerate() {
                let distance = dijkstra.run(&graph, i, j);
                assert_eq!(*result, distance);
            }
        }
    }

    #[test]
    fn retrieve_node_path() {
        let graph = create_graph();
        let mut dijkstra = UnidirectionalDijkstra::new();
        let distance = dijkstra.run(&graph, 0, 3);
        assert_eq!(9, distance);
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

        let mut dijkstra = UnidirectionalDijkstra::new();
        let distance = dijkstra.run(&graph, 0, 5);
        assert_eq!(distance, 15);
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

        let mut dijkstra = UnidirectionalDijkstra::new();
        let distance = dijkstra.run(&graph, 1, 19);
        assert_eq!(distance, 21109);
    }
}
