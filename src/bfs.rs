use crate::graph::{EdgeID, Graph, NodeID, INVALID_NODE_ID};
use bitvec::vec::BitVec;
use log::{debug, info};
use std::{collections::VecDeque, time::Instant};

pub struct BFS {
    sources: Vec<NodeID>,
    target_set: BitVec,
    parents: Vec<NodeID>,
    target: NodeID,
    queue: VecDeque<usize>,
    empty_target_set: bool,
}

impl BFS {
    // TODO: Also pass Graph instance
    pub fn new(source_list: &[NodeID], target_list: &[NodeID], number_of_nodes: usize) -> Self {
        let mut temp = Self {
            sources: source_list.to_vec(),
            target_set: BitVec::with_capacity(number_of_nodes),
            parents: Vec::new(),
            target: INVALID_NODE_ID,
            queue: VecDeque::new(),
            empty_target_set: target_list.is_empty(),
        };

        // initialize bit vector storing which nodes are targets
        temp.target_set.resize(number_of_nodes, false);
        for i in target_list {
            temp.target_set.set(*i, true);
        }

        temp.populate_sources(number_of_nodes);
        temp
    }

    fn populate_sources(&mut self, number_of_nodes: usize) {
        self.parents.resize(number_of_nodes, INVALID_NODE_ID);
        for s in &self.sources {
            self.parents[*s] = *s;
        }
    }

    pub fn run<T, G: Graph<T>>(&mut self, graph: &G) -> bool {
        self.run_with_filter(graph, |_graph, _edge| false)
    }

    /// explore the graph in a BFS
    /// returns true if a path between s and t was found or no target was given
    pub fn run_with_filter<T, F, G: Graph<T>>(&mut self, graph: &G, filter: F) -> bool
    where
        F: Fn(&G, EdgeID) -> bool,
    {
        let start = Instant::now();
        // reset queue w/o allocating
        self.queue.clear();
        self.queue.extend(self.sources.iter().copied());

        // reset parents vector
        self.parents.fill(INVALID_NODE_ID);
        for s in &self.sources {
            self.parents[*s] = *s;
        }

        while let Some(node) = self.queue.pop_front() {
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
                    // edge is fully contained within source set and we can ignore it, too
                    continue;
                }
                self.parents[target] = node;
                unsafe {
                    // unsafe is used for performance, here, as the graph is consistent by construction
                    if *self.target_set.get_unchecked(target) {
                        self.target = target;
                        debug!("setting target {}", self.target);
                        // check if we have found our target if it exists
                        let duration = start.elapsed();
                        info!("D/BFS took: {:?} (done)", duration);
                        return true;
                    }
                }
                self.queue.push_back(target);
            }
        }

        let duration = start.elapsed();
        info!("BFS took: {:?} (done)", duration);

        // return true only if target set was empty
        self.empty_target_set
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

    //TODO: Add test covering this iterator
    pub fn path_iter(&self) -> PathIter {
        PathIter::new(self)
    }
}

pub struct PathIter<'a> {
    bfs: &'a BFS,
    id: usize,
}

impl<'a> PathIter<'a> {
    pub fn new(bfs: &BFS) -> PathIter {
        debug!("init: {}", bfs.target);
        PathIter {
            bfs,
            id: bfs.target,
        }
    }
}

impl<'a> Iterator for PathIter<'a> {
    type Item = NodeID;
    fn next(&mut self) -> Option<NodeID> {
        if self.id == INVALID_NODE_ID {
            // INVALID_NODE_ID is the indicator that unpacking is done or not possible
            return None;
        }

        // path unpacking step
        let result = self.id;
        self.id = self.bfs.parents[self.id];
        if result == self.bfs.parents[result] {
            self.id = INVALID_NODE_ID;
        }
        Some(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::edge::InputEdge;
    use crate::graph::Graph;
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

        let path: Vec<usize> = bfs.path_iter().collect();
        assert_eq!(path, vec![5, 1, 0]);
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

        let path: Vec<usize> = bfs.path_iter().collect();
        assert!(path.is_empty());
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
        let mut bfs = BFS::new(&[0, 1], &[], graph.number_of_nodes());
        assert!(bfs.run(&graph));

        // path unpacking
        let path = bfs.fetch_node_path_from_node(3);
        assert_eq!(path, vec![1, 2, 3]);

        let path: Vec<usize> = bfs.path_iter().collect();
        assert!(path.is_empty());
    }
}
