/// Implementation of a unidirectional Dijkstra that uses the adresseable heap
/// as its priority queue.
///
/// The main advantage of this implementation is that it stores the entire
/// search space of each run in its internal structures. From there paths can
/// be unpacked.
use crate::{
    addressable_binary_heap::AddressableHeap,
    graph::{Graph, NodeID},
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
                } else if self.queue.weight(v) > new_distance {
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
        for i in 0..6 {
            for j in 0..6 {
                let distance = dijkstra.run(&graph, i, j);
                assert_eq!(results_table[i][j], distance);
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
}
