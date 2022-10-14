use crate::graph::Graph;

/// Returns whether the graph contains a cycle by running a node
/// coloring Depth-First-Search (DFS)
pub fn cycle_check<T>(graph: &(impl Graph<T> + 'static)) -> bool {
    #[derive(Clone, Eq, PartialEq)]
    enum Colors {
        White,
        Grey,
        Black,
    }

    let mut node_colors = vec![Colors::White; graph.number_of_nodes()];
    let mut stack = Vec::new();

    for root in graph.node_range() {
        if node_colors[root] != Colors::White {
            continue;
        }

        stack.push(root);
        while let Some(&node) = stack.last() {
            // pre-order traversal
            if node_colors[node] != Colors::Grey {
                node_colors[node] = Colors::Grey;

                for edge in graph.edge_range(node) {
                    // push unvisited children to stack
                    let target = graph.target(edge);
                    match node_colors[target] {
                        Colors::White => {
                            stack.push(target);
                        }
                        Colors::Grey => {
                            return true;
                        }
                        _ => {}
                    };
                }
            } else if node_colors[node] == Colors::Grey {
                // post-order traversal
                stack.pop();
                node_colors[node] = Colors::Black;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::edge::InputEdge;
    use crate::{cycle_check::cycle_check, static_graph::StaticGraph};

    #[test]
    fn no_cycle() {
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
        assert!(!cycle_check(&graph));
    }

    #[test]
    fn cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(2, 3, 3),
            InputEdge::new(3, 4, 1),
            InputEdge::new(4, 5, 6),
            InputEdge::new(5, 2, 2),
        ];
        let graph = Graph::new(edges);
        assert!(cycle_check(&graph));
    }
}
