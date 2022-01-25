use crate::graph::Graph;
use crate::graph::NodeID;
use core::cmp::min;

#[derive(Clone)]
struct NodeInfo {
    index: usize,
    lowlink: NodeID,
    caller: NodeID,
    vindex: usize,
}

impl NodeInfo {
    pub fn new() -> Self {
        NodeInfo {
            index: usize::MAX,
            lowlink: NodeID::MAX,
            caller: NodeID::MAX,
            vindex: usize::MAX,
        }
    }

    fn update_lowlink(&mut self, update: NodeID) {
        self.lowlink = min(self.lowlink, update);
    }
}

pub struct Tarjan {
    index: usize,
    tarjanStack: Vec<NodeID>,
    onStack: Vec<bool>,
    nodes: Vec<NodeInfo>,
}

impl Tarjan {
    pub fn new() -> Self {
        Self {
            index: 0,
            tarjanStack: Vec::new(),
            onStack: Vec::new(),
            nodes: Vec::new(),
        }
    }

    fn run<T>(&mut self, graph: &(impl Graph<T> + 'static)) -> Vec<usize> {
        let mut assignment = Vec::new();
        let mut index = 0;
        let mut num_scc = 0;
        assignment.resize(graph.number_of_nodes(), usize::MAX);
        self.nodes.resize(graph.number_of_nodes(), NodeInfo::new());
        self.onStack.resize(graph.number_of_nodes(), false);
        for n in 0..graph.number_of_nodes() {
            if self.nodes[n].index != usize::MAX {
                continue;
            }
            // TODO(dluxen): consider moving to a function
            self.nodes[n].index = index;
            self.nodes[n].lowlink = index;
            index += 1;
            self.nodes[n].vindex = 0;
            self.tarjanStack.push(n);
            self.nodes[n].caller = usize::MAX;
            self.onStack[n] = true;

            let mut last = n;
            loop {
                if self.nodes[last].vindex < graph.out_degree(last) {
                    let e = graph
                        .edge_range(last)
                        .skip(self.nodes[last].vindex)
                        .next()
                        .expect("edge range exhausted");
                    let w = graph.target(e);
                    // println!("explore ({n},{w})");
                    self.nodes[last].vindex += 1;
                    if self.nodes[w].index == usize::MAX {
                        self.nodes[w].caller = last;
                        self.nodes[w].vindex = 0;
                        self.nodes[w].index = index;
                        self.nodes[w].lowlink = index;
                        index += 1;
                        self.tarjanStack.push(w);
                        self.onStack[w] = true;
                        last = w;
                    } else if self.onStack[w] {
                        let prev_link = self.nodes[last].lowlink;
                        self.nodes[last].lowlink = min(prev_link, self.nodes[w].index);
                    }
                } else {
                    if self.nodes[last].lowlink == self.nodes[last].index {
                        num_scc += 1;
                        let mut top = self.tarjanStack.pop().expect("tarjanStack empty");
                        self.onStack[top] = false;
                        let mut size = 1;
                        assignment[top] = num_scc;
                        while top != last {
                            top = self.tarjanStack.pop().expect("tarjanStack empty");
                            self.onStack[top] = false;
                            size += 1;
                            assignment[top] = num_scc;
                        }
                        println!("detected SCC of size {size}");
                    }

                    let new_last = self.nodes[last].caller;
                    if new_last != usize::MAX {
                        self.nodes[new_last].lowlink =
                            min(self.nodes[new_last].lowlink, self.nodes[last].lowlink);
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
