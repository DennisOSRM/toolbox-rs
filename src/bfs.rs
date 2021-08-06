use std::collections::VecDeque;

use crate::graph::{Graph, NodeID, INVALID_NODE_ID};

/// explore the graph in a BFS
/// TODO(dluxen): check if source and target sets can be handled by slices
pub fn bfs<T>(
    graph: &(dyn Graph<T> + 'static),
    s: NodeID,
    t: Option<NodeID>,
    parents: &mut Vec<NodeID>,
) -> bool {
    parents.clear();
    parents.resize(graph.number_of_nodes(), INVALID_NODE_ID);

    parents[s as usize] = s;

    let mut queue = VecDeque::new();
    queue.push_front(s);

    while let Some(node) = queue.pop_back() {
        // parents[node as usize] =

        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            if parents[target as usize] != INVALID_NODE_ID {
                // we already have seen this node and can ignore it
                continue;
            }
            parents[target as usize] = node;
            if t.is_some() && t.unwrap() == target {
                // check if we have found our target if it exists
                return true;
            }
            queue.push_front(target);
        }
    }

    // return true only if all nodes should have been explored
    t.is_none()
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
        assert_eq!(true, bfs(&graph, 0, Some(5), &mut parents));

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
        assert_eq!(true, bfs(&graph, 0, None, &mut parents));

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
}
