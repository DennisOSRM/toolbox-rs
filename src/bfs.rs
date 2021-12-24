use rustc_hash::FxHashSet;
use std::collections::VecDeque;

use crate::graph::{EdgeID, Graph, NodeID, INVALID_NODE_ID};

pub struct BFS {
    sources: Vec<NodeID>,
    target_set: FxHashSet<NodeID>,
    parents: Vec<NodeID>,
    target: NodeID,

    queue: VecDeque<usize>,
}

impl BFS {
    // TODO: Also pass Graph instance
    pub fn new(source_list: &[NodeID], target_list: &[NodeID], number_of_nodes: usize) -> Self {
        let mut temp = Self {
            sources: source_list.iter().copied().collect(),
            target_set: target_list.iter().copied().collect(),
            parents: Vec::new(),
            target: INVALID_NODE_ID,
            queue: VecDeque::new(),
        };
        temp.populate_sources(number_of_nodes);
        temp
    }

    fn populate_sources(&mut self, number_of_nodes: usize) {
        self.parents
            .resize(number_of_nodes, INVALID_NODE_ID);
        for s in &self.sources {
            self.parents[*s] = *s;
        }
    }

    pub fn run<T, G: Graph<T>>(
        &mut self,
        graph: &G,
    ) -> bool {
        self.run_with_filter(graph, |_graph, _edge| false)
    }

    /// explore the graph in a BFS
    /// returns true if a path between s and t was found or no target was given
    pub fn run_with_filter<T, F, G: Graph<T>>(
        &mut self,
        graph: &G,
        filter: F,
    ) -> bool
    where
        F: Fn(&G, EdgeID) -> bool,
    {
        // reset queue
        self.queue = self.sources.iter().copied().collect();

        // reset parents vector
        self.parents.fill(INVALID_NODE_ID);
        for s in &self.sources {
            self.parents[*s] = *s;
        }

        while let Some(node) = self.queue.pop_back() {
            let node_is_source = self.parents[node] == node;
                // sources have themselves as parents
            for edge in graph.edge_range(node) {
                if filter(graph, edge) {
                    continue;
                }
                let target = graph.target(edge);
                if self.parents[target] != INVALID_NODE_ID
                    || (node_is_source && self.parents[target] == target)
                {
                    // we already have seen this node and can ignore it, or
                    // edge is fully contained within source set and we can ignore it
                    continue;
                }
                self.parents[target] = node;
                if self.target_set.contains(&target) {
                    self.target = target;
                    // check if we have found our target if it exists
                    return true;
                }
                self.queue.push_front(target);
            }
        }

        // return true only if target set was empty
        self.target_set.is_empty()
    }

    // path unpacking, by searching for the first target that was found
    // and by then unwinding the path of nodes to it.
    pub fn fetch_node_path(&self) -> Vec<NodeID> {
        self.fetch_node_path_from_node(self.target)
    }

    // path unpacking, by searching for the first target that was found
    // and by then unwinding the path of nodes to it.
    // TODO: needs test to check what happens when t is unknown, or unvisited. Can this be removed?
    pub fn fetch_node_path_from_node(&self, t: NodeID) -> Vec<NodeID> {
        let mut id = t;
        let mut path = Vec::new();
        while id != self.parents[id] {
            path.push(id);
            id = self.parents[id];
        }
        path.push(id);
        path.reverse();
        path
    }

    // TODO: the reverse might be unnecessary to some applications
    pub fn fetch_edge_path<T>(&self, graph: &impl Graph<T>) -> Vec<EdgeID> {
        // path unpacking
        let mut id = self.target;
        let mut path = Vec::new();
        while id != self.parents[id] {
            let edge_id = graph.find_edge(self.parents[id], id).unwrap();
            path.push(edge_id);
            id = self.parents[id];
        }

        path.reverse();
        path
    }
}

#[cfg(test)]
mod tests {
    use crate::graph::Graph;
use crate::edge::InputEdge;
    use crate::{bfs::BFS, static_graph::StaticGraph};

    #[test]
    fn s_t_query_fetch_node_string() {
        type Graph = StaticGraph<i32>;
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
        let graph = Graph::new(edges);
        let mut bfs = BFS::new(&[0], &[5], graph.number_of_nodes());
        assert!(bfs.run(&graph));

        let path = bfs.fetch_node_path();
        assert_eq!(path, vec![0, 1, 5]);
    }

    #[test]
    fn s_t_query_edge_list() {
        type Graph = StaticGraph<i32>;
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
        let graph = Graph::new(edges);
        let mut bfs = BFS::new(&[0], &[5], graph.number_of_nodes());
        assert!(bfs.run(&graph));
        let path = bfs.fetch_edge_path(&graph);
        assert_eq!(path, vec![0, 3]);
    }

    #[test]
    fn s_all_query() {
        type Graph = StaticGraph<i32>;
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
        let graph = Graph::new(edges);
        let mut bfs = BFS::new(&[0], &[], graph.number_of_nodes());
        assert!(bfs.run(&graph));

        let path = bfs.fetch_node_path_from_node(3);
        assert_eq!(path, vec![0, 1, 2, 3]);
    }

    #[test]
    fn multi_s_all_query() {
        type Graph = StaticGraph<i32>;
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
        let graph = Graph::new(edges);
        let mut bfs = BFS::new( &[0, 1], &[], graph.number_of_nodes());
        assert!(bfs.run(&graph));

        // path unpacking
        let path = bfs.fetch_node_path_from_node(3);
        assert_eq!(path, vec![1, 2, 3]);
    }
}
