use crate::graph::{Graph, NodeID};
use core::cmp::min;

#[derive(Clone)]
struct DFSNode {
    index: usize,
    lowlink: NodeID,
    caller: NodeID,
    neighbor: usize,
    on_stack: bool,
}

impl DFSNode {
    pub fn new() -> Self {
        DFSNode {
            index: usize::MAX,
            lowlink: NodeID::MAX,
            caller: NodeID::MAX,
            neighbor: usize::MAX,
            on_stack: false,
        }
    }
}

// TODO: consider making this a function
// TODO: consider adding handlers for small/large SCCs
pub struct Tarjan {
    tarjan_stack: Vec<NodeID>,
    dfs_state: Vec<DFSNode>,
}

impl Default for Tarjan {
    fn default() -> Self {
        Self::new()
    }
}

impl Tarjan {
    pub fn new() -> Self {
        Self {
            tarjan_stack: Vec::new(),
            dfs_state: Vec::new(),
        }
    }

    pub fn run<T>(&mut self, graph: &(impl Graph<T> + 'static)) -> Vec<usize> {
        let mut assignment = Vec::new();
        assignment.resize(graph.number_of_nodes(), usize::MAX);
        let mut index = 0;
        let mut num_scc = 0;

        // TODO: does this need to be a struct member?
        self.dfs_state
            .resize(graph.number_of_nodes(), DFSNode::new());

        // assign each node to an SCC if not yet done
        for n in graph.node_range() {
            if self.dfs_state[n].index != usize::MAX {
                continue;
            }
            // TODO: consider moving the following to a function to save indentation

            // TODO: could setting the state be done in a cleaner way?
            self.dfs_state[n].caller = usize::MAX; // marker denoting the end of recursion
            self.dfs_state[n].neighbor = 0;
            self.dfs_state[n].index = index;
            self.dfs_state[n].lowlink = index;
            self.dfs_state[n].on_stack = true;
            self.tarjan_stack.push(n);
            index += 1;
            let mut last = n;

            loop {
                if self.dfs_state[last].neighbor < graph.out_degree(last) {
                    let e = graph
                        .edge_range(last)
                        .nth(self.dfs_state[last].neighbor)
                        .expect("edge range exhausted");
                    let w = graph.target(e);
                    self.dfs_state[last].neighbor += 1;
                    if self.dfs_state[w].index == usize::MAX {
                        self.dfs_state[w].caller = last;
                        self.dfs_state[w].neighbor = 0;
                        self.dfs_state[w].index = index;
                        self.dfs_state[w].lowlink = index;
                        self.dfs_state[w].on_stack = true;
                        self.tarjan_stack.push(w);
                        index += 1;
                        last = w;
                    } else if self.dfs_state[w].on_stack {
                        self.dfs_state[last].lowlink =
                            min(self.dfs_state[last].lowlink, self.dfs_state[w].index);
                    }
                } else {
                    if self.dfs_state[last].lowlink == self.dfs_state[last].index {
                        num_scc += 1;
                        let mut size = 0;
                        loop {
                            let top = self.tarjan_stack.pop().expect("tarjan_stack empty");
                            self.dfs_state[top].on_stack = false;
                            size += 1;
                            assignment[top] = num_scc;
                            if top == last {
                                break;
                            }
                        }
                        // TODO: add handler for small/large SCCs
                        println!("detected SCC of size {size}");
                    }

                    let new_last = self.dfs_state[last].caller;
                    if new_last != usize::MAX {
                        self.dfs_state[new_last].lowlink = min(
                            self.dfs_state[new_last].lowlink,
                            self.dfs_state[last].lowlink,
                        );
                        last = new_last;
                    } else {
                        debug_assert!(n == last);
                        break;
                    }
                }
            }
        }
        assignment
    }
}

#[cfg(test)]
mod tests {
    use crate::edge::InputEdge;
    use crate::static_graph::StaticGraph;
    use crate::tarjan::Tarjan;

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

        let mut tarjan = Tarjan::new();
        assert_eq!(vec![3, 3, 2, 2, 3, 1, 1, 2], tarjan.run(&graph));
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

        let mut tarjan = Tarjan::new();
        assert_eq!(vec![3, 3, 3, 2, 1], tarjan.run(&graph));
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

        let mut tarjan = Tarjan::new();
        assert_eq!(vec![3, 1, 2, 3, 1, 2, 3, 1, 2], tarjan.run(&graph));
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

        let mut tarjan = Tarjan::new();
        assert_eq!(vec![6, 5, 3, 4, 5, 2, 1, 1, 1, 1, 1, 1], tarjan.run(&graph));
    }
}
