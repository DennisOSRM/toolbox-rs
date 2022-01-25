use crate::graph::Graph;
use crate::graph::NodeID;
use core::cmp::min;

#[derive(Clone)]
struct NodeInfo {
    index: usize,
    lowlink: NodeID,
    caller: NodeID,
    neighbor: usize,
    on_stack: bool,
}

impl NodeInfo {
    pub fn new() -> Self {
        NodeInfo {
            index: usize::MAX,
            lowlink: NodeID::MAX,
            caller: NodeID::MAX,
            neighbor: usize::MAX,
            on_stack: false,
        }
    }
}

pub struct Tarjan {
    tarjan_stack: Vec<NodeID>,
    dfs_state: Vec<NodeInfo>,
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
        let mut index = 0;
        let mut num_scc = 0;
        assignment.resize(graph.number_of_nodes(), usize::MAX);
        self.dfs_state
            .resize(graph.number_of_nodes(), NodeInfo::new());

        // assign each node to an SCC if not yet done
        for n in 0..graph.number_of_nodes() {
            if self.dfs_state[n].index != usize::MAX {
                continue;
            }
            // TODO: consider moving to a function
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
}
