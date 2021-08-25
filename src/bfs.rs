use std::{
    collections::{HashSet, VecDeque},
    marker::PhantomData,
};

use crate::graph::{EdgeID, Graph, NodeID, INVALID_NODE_ID};

pub struct BFS<'a, T, G: Graph<T>> {
    graph: &'a G,
    parents: Vec<EdgeID>,
    target: NodeID,
    dummy: PhantomData<T>,
}

impl<'a, T, G: Graph<T>> BFS<'a, T, G> {
    pub fn new(graph: &'a G) -> Self {
        Self {
            graph,
            parents: Vec::new(),
            target: INVALID_NODE_ID,
            dummy: PhantomData,
        }
    }

    pub fn run(&mut self, sources: Vec<NodeID>, targets: Vec<NodeID>) -> bool {
        self.run_with_filter(sources, targets, |_edge| false)
    }

    /// explore the graph in a BFS
    /// returns true if a path between s and t was found or no target was given
    // todo(dluxen): introduce node set macro
    // todo(dluxen): convert to struct with run(.) and retrieve_path(.) function
    // todo(dluxen): retrieve edge list rather than string of nodes
    pub fn run_with_filter<F>(
        &mut self,
        sources: Vec<NodeID>,
        targets: Vec<NodeID>,
        filter: F,
    ) -> bool
    where
        F: Fn(EdgeID) -> bool,
    {
        self.parents.clear();
        self.parents
            .resize(self.graph.number_of_nodes(), INVALID_NODE_ID);

        let target_set: HashSet<u32> = targets.into_iter().collect();

        let mut queue = VecDeque::new();
        for s in sources {
            self.parents[s as usize] = s;
            queue.push_front(s);
        }

        while let Some(node) = queue.pop_back() {
            for edge in self.graph.edge_range(node) {
                if filter(edge) {
                    continue;
                }
                let target = self.graph.target(edge);
                if self.parents[target as usize] != INVALID_NODE_ID {
                    // we already have seen this node and can ignore it
                    continue;
                }
                self.parents[target as usize] = node;
                if target_set.contains(&target) {
                    self.target = target;
                    // check if we have found our target if it exists
                    return true;
                }
                queue.push_front(target);
            }
        }

        // return true only if all nodes should have been explored
        target_set.is_empty()
    }

    // path unpacking, by searching for the first target that was found
    // and by then unwinding the path of nodes to it.
    pub fn fetch_node_path(&self) -> Vec<NodeID> {
        let mut id = self.target;
        let mut path = Vec::new();
        while id != self.parents[id as usize] {
            path.push(id);
            id = self.parents[id as usize];
        }
        path.push(id);
        path.reverse();
        path
    }

    // path unpacking, by searching for the first target that was found
    // and by then unwinding the path of nodes to it.
    pub fn fetch_node_path_from_node(&self, t: NodeID) -> Vec<NodeID> {
        let mut id = t;
        let mut path = Vec::new();
        while id != self.parents[id as usize] {
            path.push(id);
            id = self.parents[id as usize];
        }
        path.push(id);
        path.reverse();
        path
    }

    pub fn fetch_edge_path(&self) -> Vec<EdgeID> {
        // path unpacking
        let mut id = self.target;
        let mut path = Vec::new();
        while id != self.parents[id as usize] {
            let edge_id = self.graph.find_edge(self.parents[id as usize], id).unwrap();
            path.push(edge_id);
            id = self.parents[id as usize];
        }

        path.reverse();
        path
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bfs::BFS,
        static_graph::{InputEdge, StaticGraph},
    };

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
        let mut bfs = BFS::new(&graph);
        assert_eq!(true, bfs.run(vec![0], vec![5]));

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
        let mut bfs = BFS::new(&graph);
        assert_eq!(true, bfs.run(vec![0], vec![5]));
        let path = bfs.fetch_edge_path();
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
        let mut bfs = BFS::new(&graph);
        assert_eq!(true, bfs.run(vec![0], vec![]));

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
        let mut bfs = BFS::new(&graph);
        assert_eq!(true, bfs.run(vec![0, 1], vec![]));

        // path unpacking
        let path = bfs.fetch_node_path_from_node(3);
        assert_eq!(path, vec![1, 2, 3]);
    }
}
