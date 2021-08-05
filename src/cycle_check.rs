/// Returns whether the graph contains a cycle by running a node
/// coloring DFS
pub fn cycle_check(&self) -> bool {
    #[derive(Clone, PartialEq)]
    enum Colors {
        White,
        Grey,
        Black,
    }

    let mut node_colors = Vec::new();
    node_colors.resize(self.number_of_nodes(), Colors::White);
    let mut stack = Vec::new();

    for root in self.node_range() {
        if node_colors[root as usize] != Colors::White {
            continue;
        }

        stack.push(root);
        while let Some(&node) = stack.last() {
            // pre-order traversal
            if node_colors[node as usize] != Colors::Grey {
                node_colors[node as usize] = Colors::Grey;

                for edge in self.edge_range(node) {
                    // push unvisited children to stack
                    let target = self.target(edge);

                    if node_colors[target as usize] == Colors::White {
                        stack.push(target);
                    } else if node_colors[target as usize] == Colors::Grey {
                        // cycle detected
                        return true;
                    }
                }
            } else if node_colors[node as usize] == Colors::Grey {
                // post-order traversal
                stack.pop();
                node_colors[node as usize] = Colors::Black;
            }
        }
    }
    false
}

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
    assert_eq!(false, graph.cycle_check());
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
    assert_eq!(true, graph.cycle_check());
}
