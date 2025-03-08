/* 
 * Path-based SCC algorithm (Gabow's Algorithm)
 * 
 * The algorithm numbers SCCs in reverse order of their discovery:
 * - Initially, component counter starts at n (number of vertices)
 * - Each time an SCC is found, counter is decremented
 * - Final numbering is from n-1 down to 0
 * 
 * This approach serves multiple purposes:
 * 1. Avoids conflicts with temporary stack indices during processing
 * 2. Creates a reverse topological ordering of SCCs
 * 3. Ensures each SCC gets a unique, consecutive number
 * 
 * Example: In a graph with 5 vertices and 3 SCCs:
 * - First discovered SCC gets number 4
 * - Second SCC gets number 3
 * - Last SCC gets number 2
 */

use crate::graph::Graph;

#[derive(Clone, Copy)]
enum DfsState {
    Visit(usize),             // Knoten zum ersten Mal besuchen
    ProcessNeighbors(usize),  // Nachbarn verarbeiten
    Finalize(usize),         // SCC finalisieren
}

pub struct PathBasedScc {
    scc: Vec<usize>,
    stack: Vec<usize>,  // Stack S
    bounds: Vec<usize>, // Stack B
    component: usize,
}

impl Default for PathBasedScc {
    fn default() -> Self {
        Self::new()
    }
}

impl PathBasedScc {
    pub fn new() -> Self {
        Self {
            bounds: Vec::new(),
            scc: Vec::new(),
            stack: Vec::new(),
            component: 0,
        }
    }

    pub fn run<T>(&mut self, graph: &(impl Graph<T> + 'static)) -> Vec<usize> {
        // initialization
        self.bounds = Vec::new();
        self.scc.resize(graph.number_of_nodes(), usize::MAX);
        self.stack = Vec::new();
        self.component = graph.number_of_nodes();

        // main loop
        for v in graph.node_range() {
            if self.scc[v] == usize::MAX {
                self.dfs_iterative(v, graph);
            }
        }

        self.scc.clone()
    }

    fn dfs_iterative<T>(&mut self, start: usize, graph: &(impl Graph<T> + 'static)) {
        let mut dfs_stack = vec![DfsState::Visit(start)];
        let mut edge_indices = vec![0usize; graph.number_of_nodes()];

        while let Some(state) = dfs_stack.pop() {
            match state {
                DfsState::Visit(v) => {
                    // step 1: push v onto S
                    self.stack.push(v);
                    self.scc[v] = self.stack.len() - 1;
                    self.bounds.push(self.scc[v]);
                    dfs_stack.push(DfsState::ProcessNeighbors(v));
                }

                DfsState::ProcessNeighbors(v) => {
                    let edges: Vec<_> = graph.edge_range(v).collect();
                    if edge_indices[v] < edges.len() {
                        let e = edges[edge_indices[v]];
                        edge_indices[v] += 1;
                        dfs_stack.push(DfsState::ProcessNeighbors(v));

                        let w = graph.target(e);
                        if self.scc[w] == usize::MAX {
                            dfs_stack.push(DfsState::Visit(w));
                        } else {
                            // contract if necessary
                            while let Some(&bound) = self.bounds.last() {
                                if self.scc[w] < bound {
                                    self.bounds.pop();
                                } else {
                                    break;
                                }
                            }
                        }
                    } else {
                        dfs_stack.push(DfsState::Finalize(v));
                    }
                }

                DfsState::Finalize(v) => {
                    if Some(&self.scc[v]) == self.bounds.last() {
                        self.bounds.pop();
                        self.component -= 1;
                        while let Some(u) = self.stack.pop() {
                            self.scc[u] = self.component;
                            if u == v {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::edge::InputEdge;
    use crate::path_based_scc::PathBasedScc;
    use crate::static_graph::StaticGraph;

    #[test]
    fn scc_wiki1() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(1, 4, 1),
            InputEdge::new(1, 5, 6),
            InputEdge::new(2, 3, 2),
            InputEdge::new(2, 6, 2),
            InputEdge::new(3, 2, 2),
            InputEdge::new(3, 7, 2),
            InputEdge::new(4, 0, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 6, 2),
            InputEdge::new(6, 5, 2),
            InputEdge::new(7, 3, 2),
            InputEdge::new(7, 6, 2),
        ];
        let graph = Graph::new(edges);

        let mut scc = PathBasedScc::new();
        assert_eq!(vec![5, 5, 6, 6, 5, 7, 7, 6], scc.run(&graph));
    }

    #[test]
    fn geekforgeeks() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(1, 0, 3),
            InputEdge::new(0, 3, 3),
            InputEdge::new(0, 2, 1),
            InputEdge::new(2, 1, 6),
            InputEdge::new(3, 4, 2),
        ];
        let graph = Graph::new(edges);

        let mut scc = PathBasedScc::new();
        assert_eq!(vec![2, 2, 2, 3, 4], scc.run(&graph));
    }

    #[test]
    fn stanford2() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 6, 3),
            InputEdge::new(6, 3, 3),
            InputEdge::new(3, 0, 1),
            InputEdge::new(6, 8, 6),
            InputEdge::new(8, 5, 2),
            InputEdge::new(5, 2, 2),
            InputEdge::new(2, 8, 2),
            InputEdge::new(5, 7, 2),
            InputEdge::new(7, 1, 2),
            InputEdge::new(4, 7, 2),
            InputEdge::new(1, 4, 2),
        ];
        let graph = Graph::new(edges);

        let mut scc = PathBasedScc::new();
        assert_eq!(vec![6, 8, 7, 6, 8, 7, 6, 8, 7], scc.run(&graph));
    }

    #[test]
    fn web1() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 3, 3),
            InputEdge::new(1, 4, 1),
            InputEdge::new(1, 2, 6),
            InputEdge::new(2, 5, 2),
            InputEdge::new(4, 1, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(4, 6, 2),
            InputEdge::new(5, 7, 2),
            InputEdge::new(6, 7, 2),
            InputEdge::new(6, 8, 2),
            InputEdge::new(7, 9, 2),
            InputEdge::new(9, 10, 2),
            InputEdge::new(10, 8, 2),
            InputEdge::new(8, 11, 2),
            InputEdge::new(11, 6, 2),
        ];
        let graph = Graph::new(edges);

        let mut scc = PathBasedScc::new();
        assert_eq!(vec![6, 7, 9, 8, 7, 10, 11, 11, 11, 11, 11, 11], scc.run(&graph));
    }
}
