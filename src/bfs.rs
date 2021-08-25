use std::collections::{HashSet, VecDeque};

use crate::graph::{EdgeID, Graph, NodeID, INVALID_NODE_ID};

pub fn bfs<T>(
    graph: &(impl Graph<T> + 'static),
    // todo(dluxen): ^^ run experiment on dyn vs impl
    sources: Vec<NodeID>,
    targets: Vec<NodeID>,
    parents: &mut Vec<NodeID>,
) -> bool {
    bfs_with_filter(graph, sources, targets, parents, |_edge| false)
}
/// explore the graph in a BFS
/// returns true if a path between s and t was found or no target was given
// todo(dluxen): introduce node set macro
// todo(dluxen): convert to struct with run(.) and retrieve_path(.) function
// todo(dluxen): retrieve edge list rather than string of nodes
pub fn bfs_with_filter<T, F>(
    graph: &(impl Graph<T> + 'static),
    // todo(dluxen): ^^ run experiment on dyn vs impl
    sources: Vec<NodeID>,
    targets: Vec<NodeID>,
    parents: &mut Vec<NodeID>,
    filter: F,
) -> bool
where
    F: Fn(EdgeID) -> bool,
{
    parents.clear();
    parents.resize(graph.number_of_nodes(), INVALID_NODE_ID);

    let target_set: HashSet<u32> = targets.into_iter().collect();

    let mut queue = VecDeque::new();
    for s in sources {
        parents[s as usize] = s;
        queue.push_front(s);
    }

    while let Some(node) = queue.pop_back() {
        for edge in graph.edge_range(node) {
            if filter(edge) {
                continue;
            }
            let target = graph.target(edge);
            if parents[target as usize] != INVALID_NODE_ID {
                // we already have seen this node and can ignore it
                continue;
            }
            parents[target as usize] = node;
            if target_set.contains(&target) {
                // check if we have found our target if it exists
                return true;
            }
            queue.push_front(target);
        }
    }

    // return true only if all nodes should have been explored
    target_set.is_empty()
}

#[cfg(test)]
mod tests {
    use crate::{
        bfs::bfs,
        static_graph::{InputEdge, StaticGraph},
    };

    #[test]
    fn s_t_query() {
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
        let mut parents = Vec::new();
        assert_eq!(true, bfs(&graph, vec![0], vec![5], &mut parents));

        // path unpacking
        // TODO(dluxen): move to function?
        let mut id = 5;
        let mut path = Vec::new();
        while id != parents[id as usize] {
            path.push(id);
            id = parents[id as usize];
        }
        path.push(id);
        path.reverse();
        assert_eq!(path, vec![0, 1, 5]);
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
        let mut parents = Vec::new();
        assert_eq!(true, bfs(&graph, vec![0], vec![], &mut parents));

        // path unpacking
        let mut id = 3;
        let mut path = Vec::new();
        while id != parents[id as usize] {
            path.push(id);
            id = parents[id as usize];
        }
        path.push(id);
        path.reverse();
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
        let mut parents = Vec::new();
        assert_eq!(true, bfs(&graph, vec![0, 1], vec![], &mut parents));

        // path unpacking
        let mut id = 3;
        let mut path = Vec::new();
        while id != parents[id as usize] {
            path.push(id);
            id = parents[id as usize];
        }
        path.push(id);
        path.reverse();
        assert_eq!(path, vec![1, 2, 3]);
    }
}
