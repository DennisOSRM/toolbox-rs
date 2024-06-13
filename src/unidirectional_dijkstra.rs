/// Implementation of a unidirectional Dijkstra that uses the adresseable heap
/// as its priority queue.
///
/// The main advantage of this implementation is that it stores the entire
/// search space of each run in its internal structures. From there paths can
/// be unpacked.
use crate::{
    addressable_binary_heap::AddressableHeap,
    graph::{Graph, NodeID},
    search_space::SearchSpace,
};

use log::debug;

pub struct UnidirectionalDijkstra {
    queue: AddressableHeap<NodeID, usize, NodeID>,
    upper_bound: usize,
}

impl Default for UnidirectionalDijkstra {
    fn default() -> Self {
        Self::new()
    }
}

impl UnidirectionalDijkstra {
    pub fn new() -> Self {
        let queue = AddressableHeap::<NodeID, usize, NodeID>::new();
        Self {
            queue,
            upper_bound: usize::MAX,
        }
    }

    /// clears the search space stored in the queue.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.upper_bound = usize::MAX;
    }

    /// retrieves the number of nodes that were explored (not settled) during
    /// a search.
    pub fn search_space_len(&self) -> usize {
        self.queue.inserted_len()
    }

    /// run a path computation from s to t on some graph. The object is reusable
    /// to run consecutive searches, even on different graphs. It is cleared on
    /// every run, which saves on allocations.
    pub fn run<G: Graph<usize>>(&mut self, graph: &G, s: NodeID, t: NodeID) -> usize {
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
                let new_distance = distance + *graph.data(edge);

                if !self.queue.inserted(v) {
                    debug!("[push] node: {v}, weight: {new_distance}, parent: {u}");
                    // if target not enqued before, do now
                    self.queue.insert(v, new_distance, u);
                }
                if self.queue.contains(v) && self.queue.weight(v) > new_distance {
                    debug!("[decrease] node: {v}, new weight: {new_distance}, new parent: {u}");
                    // if lower distance found, update distance and its parent
                    self.queue.decrease_key_and_update_data(v, new_distance, v);
                }
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
            let parent = *self.queue.data(node);
            if parent == node {
                // reverse order to go from source to target
                path.reverse();
                return Some(path);
            }
            path.push(parent);
            node = parent;
        }
    }

    pub fn search_space(&self) -> SearchSpace {
        SearchSpace::new(&self.queue)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        edge::InputEdge, graph::Graph, static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    fn create_graph() -> StaticGraph<usize> {
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];
        let graph = StaticGraph::<usize>::new(edges);
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());

        graph
    }

    #[test]
    fn simple_graph() {
        let graph = create_graph();

        let mut dijkstra = UnidirectionalDijkstra::new();
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
            InputEdge::new(0, 1, 7),
            InputEdge::new(0, 2, 3),
            InputEdge::new(1, 2, 1),
            InputEdge::new(1, 3, 6),
            InputEdge::new(2, 4, 8),
            InputEdge::new(3, 5, 2),
            InputEdge::new(4, 3, 2),
            InputEdge::new(4, 5, 8),
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
            InputEdge::new(3, 12, 2852),
            InputEdge::new(3, 13, 1641),
            InputEdge::new(3, 26, 1334),
            InputEdge::new(3, 14, 425),
            InputEdge::new(3, 27, 1380),
            InputEdge::new(28, 29, 2713),
            InputEdge::new(28, 30, 2378),
            InputEdge::new(28, 31, 1114),
            InputEdge::new(28, 8, 1013),
            InputEdge::new(32, 30, 1225),
            InputEdge::new(32, 33, 892),
            InputEdge::new(32, 31, 2375),
            InputEdge::new(34, 33, 2497),
            InputEdge::new(34, 35, 885),
            InputEdge::new(34, 31, 1332),
            InputEdge::new(36, 37, 2886),
            InputEdge::new(36, 38, 864),
            InputEdge::new(36, 39, 126),
            InputEdge::new(37, 36, 2886),
            InputEdge::new(38, 36, 864),
            InputEdge::new(38, 40, 3560),
            InputEdge::new(38, 41, 1770),
            InputEdge::new(38, 42, 826),
            InputEdge::new(40, 38, 3560),
            InputEdge::new(40, 39, 3335),
            InputEdge::new(40, 43, 2295),
            InputEdge::new(41, 38, 1770),
            InputEdge::new(1, 15, 667),
            InputEdge::new(1, 44, 901),
            InputEdge::new(1, 9, 1233),
            InputEdge::new(44, 1, 901),
            InputEdge::new(45, 46, 1638),
            InputEdge::new(45, 47, 889),
            InputEdge::new(45, 48, 2582),
            InputEdge::new(46, 45, 1638),
            InputEdge::new(47, 45, 889),
            InputEdge::new(47, 49, 1311),
            InputEdge::new(47, 11, 508),
            InputEdge::new(49, 47, 1311),
            InputEdge::new(11, 47, 508),
            InputEdge::new(11, 7, 3106),
            InputEdge::new(11, 50, 1979),
            InputEdge::new(11, 16, 1334),
            InputEdge::new(4, 26, 1917),
            InputEdge::new(4, 51, 859),
            InputEdge::new(4, 17, 1140),
            InputEdge::new(4, 2, 2888),
            InputEdge::new(4, 52, 1885),
            InputEdge::new(26, 3, 1334),
            InputEdge::new(26, 4, 1917),
            InputEdge::new(26, 51, 1657),
            InputEdge::new(51, 4, 859),
            InputEdge::new(51, 26, 1657),
            InputEdge::new(51, 53, 1253),
            InputEdge::new(51, 54, 2474),
            InputEdge::new(27, 3, 1380),
            InputEdge::new(27, 53, 690),
            InputEdge::new(27, 8, 3284),
            InputEdge::new(2, 18, 1249),
            InputEdge::new(2, 4, 2888),
            InputEdge::new(2, 55, 1560),
            InputEdge::new(52, 4, 1885),
            InputEdge::new(52, 55, 1525),
            InputEdge::new(52, 56, 2467),
            InputEdge::new(53, 51, 1253),
            InputEdge::new(53, 27, 690),
            InputEdge::new(53, 29, 552),
            InputEdge::new(29, 28, 2713),
            InputEdge::new(29, 53, 552),
            InputEdge::new(29, 57, 1196),
            InputEdge::new(0, 19, 2224),
            InputEdge::new(0, 5, 584),
            InputEdge::new(0, 58, 2113),
            InputEdge::new(0, 59, 1065),
            InputEdge::new(5, 20, 491),
            InputEdge::new(5, 0, 584),
            InputEdge::new(5, 60, 904),
            InputEdge::new(60, 5, 904),
            InputEdge::new(60, 30, 1111),
            InputEdge::new(60, 8, 2549),
            InputEdge::new(58, 0, 2113),
            InputEdge::new(58, 30, 491),
            InputEdge::new(58, 61, 2112),
            InputEdge::new(59, 0, 1065),
            InputEdge::new(59, 62, 983),
            InputEdge::new(59, 63, 4556),
            InputEdge::new(30, 28, 2378),
            InputEdge::new(30, 32, 1225),
            InputEdge::new(30, 60, 1111),
            InputEdge::new(30, 58, 491),
            InputEdge::new(61, 58, 2112),
            InputEdge::new(61, 33, 573),
            InputEdge::new(61, 63, 1038),
            InputEdge::new(61, 64, 3897),
            InputEdge::new(33, 32, 892),
            InputEdge::new(33, 34, 2497),
            InputEdge::new(33, 61, 573),
            InputEdge::new(62, 59, 983),
            InputEdge::new(62, 39, 1070),
            InputEdge::new(62, 65, 5245),
            InputEdge::new(63, 59, 4556),
            InputEdge::new(63, 61, 1038),
            InputEdge::new(63, 65, 1544),
            InputEdge::new(63, 66, 3563),
            InputEdge::new(39, 36, 126),
            InputEdge::new(39, 40, 3335),
            InputEdge::new(39, 62, 1070),
            InputEdge::new(42, 38, 826),
            InputEdge::new(42, 67, 672),
            InputEdge::new(42, 6, 989),
            InputEdge::new(67, 42, 672),
            InputEdge::new(6, 42, 989),
            InputEdge::new(6, 21, 424),
            InputEdge::new(55, 2, 1560),
            InputEdge::new(55, 52, 1525),
            InputEdge::new(55, 68, 2967),
            InputEdge::new(56, 52, 2467),
            InputEdge::new(56, 35, 414),
            InputEdge::new(56, 54, 1016),
            InputEdge::new(35, 34, 885),
            InputEdge::new(35, 56, 414),
            InputEdge::new(35, 68, 1242),
            InputEdge::new(48, 45, 2582),
            InputEdge::new(48, 69, 828),
            InputEdge::new(48, 64, 1589),
            InputEdge::new(48, 70, 1657),
            InputEdge::new(69, 48, 828),
            InputEdge::new(69, 7, 371),
            InputEdge::new(69, 71, 861),
            InputEdge::new(7, 11, 3106),
            InputEdge::new(7, 69, 371),
            InputEdge::new(7, 22, 742),
            InputEdge::new(65, 62, 5245),
            InputEdge::new(65, 63, 1544),
            InputEdge::new(65, 43, 1306),
            InputEdge::new(66, 63, 3563),
            InputEdge::new(66, 64, 1202),
            InputEdge::new(66, 10, 997),
            InputEdge::new(43, 40, 2295),
            InputEdge::new(43, 65, 1306),
            InputEdge::new(64, 61, 3897),
            InputEdge::new(64, 48, 1589),
            InputEdge::new(64, 66, 1202),
            InputEdge::new(64, 70, 1667),
            InputEdge::new(10, 66, 997),
            InputEdge::new(10, 72, 616),
            InputEdge::new(10, 23, 1463),
            InputEdge::new(57, 29, 1196),
            InputEdge::new(57, 31, 1970),
            InputEdge::new(57, 54, 508),
            InputEdge::new(31, 28, 1114),
            InputEdge::new(31, 32, 2375),
            InputEdge::new(31, 34, 1332),
            InputEdge::new(31, 57, 1970),
            InputEdge::new(54, 51, 2474),
            InputEdge::new(54, 56, 1016),
            InputEdge::new(54, 57, 508),
            InputEdge::new(8, 28, 1013),
            InputEdge::new(8, 27, 3284),
            InputEdge::new(8, 60, 2549),
            InputEdge::new(8, 24, 1003),
            InputEdge::new(9, 1, 1233),
            InputEdge::new(9, 25, 1229),
            InputEdge::new(9, 70, 7863),
            InputEdge::new(68, 55, 2967),
            InputEdge::new(68, 35, 1242),
            InputEdge::new(68, 70, 2667),
            InputEdge::new(70, 48, 1657),
            InputEdge::new(70, 64, 1667),
            InputEdge::new(70, 9, 7863),
            InputEdge::new(70, 68, 2667),
            InputEdge::new(71, 69, 861),
            InputEdge::new(72, 10, 616),
            InputEdge::new(50, 11, 1979),
        ];
        let graph = StaticGraph::new(edges);

        let mut dijkstra = UnidirectionalDijkstra::new();
        let distance = dijkstra.run(&graph, 1, 19);
        assert_eq!(distance, 21109);
    }
}
