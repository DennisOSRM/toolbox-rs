use crate::bfs::BFS;
use crate::edge::Edge;
use crate::edge::InputEdge;
use crate::graph::{Graph, NodeID};
use crate::static_graph::StaticGraph;
use bitvec::vec::BitVec;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeCapacity {
    pub capacity: i32,
}

impl EdgeCapacity {
    pub fn new(capacity: i32) -> EdgeCapacity {
        EdgeCapacity { capacity }
    }
}

pub struct FordFulkerson {
    residual_graph: StaticGraph<EdgeCapacity>,
    max_flow: i32,
    finished: bool,
}

impl FordFulkerson {
    // todo(dl): add closure parameter to derive edge data
    pub fn from_generic_edge_list(input_edges: Vec<impl Edge<ID = NodeID>>) -> Self {
        let edge_list: Vec<InputEdge<EdgeCapacity>> = input_edges
            .into_iter()
            .map(|edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: EdgeCapacity::new(1),
            })
            .collect();

        println!("created {} ff edges", edge_list.len());
        FordFulkerson::from_edge_list(edge_list)
    }

    pub fn from_edge_list(mut edge_list: Vec<InputEdge<EdgeCapacity>>) -> Self {
        let number_of_edges = edge_list.len();

        println!("extending {} edges", edge_list.len());
        // blindly generate reverse edges for all edges with zero capacity
        edge_list.extend_from_within(..);
        edge_list.iter_mut().skip(number_of_edges).for_each(|edge| {
            edge.reverse();
            edge.data.capacity = 0;
        });
        println!("into {} edges", edge_list.len());

        // dedup-merge edge set, by using the following trick: not the dedup(.) call
        // below takes the second argument as mut. When deduping equivalent values
        // a and b, then a is accumulated onto b.
        edge_list.sort();
        edge_list.dedup_by(|a, mut b| {
            // edges a and b are assumed to be equivalent in the residual graph if
            // (and only if) they are parallel. In other words, this removes parallel
            // edges in the residual graph and accumulates capacities on the remaining
            // egde.
            let result = a.source == b.source && a.target == b.target;
            if result {
                b.data.capacity += a.data.capacity;
            }
            result
        });
        println!("dedup-merged {} edges", edge_list.len());

        // at this point the edge set of the residual graph doesn't have any
        // duplicates anymore. note that this is fine, as we are looking to
        // compute a node partition.
        Self {
            residual_graph: StaticGraph::new(edge_list),
            max_flow: 0,
            finished: false,
        }
    }

    pub fn run(&mut self, sources: &[NodeID], targets: &[NodeID]) {
        let mut bfs = BFS::new();
        let filter = |graph: &StaticGraph<EdgeCapacity>, edge| graph.data(edge).capacity <= 0;
        let mut iteration = 0;
        while bfs.run_with_filter(&self.residual_graph, sources, targets, filter) {
            iteration += 1;

            println!("iteration {}", iteration);
            // retrieve node path. This is sufficient, as we removed all duplicate edges
            let path = bfs.fetch_node_path();
            println!("found node path of size {:#?}", path.len());

            // find min capacity on edges of the path
            let st_tuple = path
                .windows(2)
                .min_by_key(|window| {
                    let edge = self.residual_graph.find_edge(window[0], window[1]).unwrap();
                    self.residual_graph.data(edge).capacity
                })
                .unwrap();

            let bottleneck_edge = self
                .residual_graph
                .find_edge(st_tuple[0], st_tuple[1])
                .unwrap();
            // println!("  bottleneck edge: {}", bottleneck_edge);
            let path_flow = self.residual_graph.data(bottleneck_edge).capacity;
            assert!(path_flow > 0);
            println!("min edge: {}, capacity: {}", bottleneck_edge, path_flow);
            // sum up flow
            self.max_flow += path_flow;

            // assign flow to residual graph
            path.windows(2).for_each(|pair| {
                let fwd_edge = self.residual_graph.find_edge(pair[0], pair[1]).unwrap();
                let rev_edge = self.residual_graph.find_edge(pair[1], pair[0]).unwrap();

                self.residual_graph.data_mut(fwd_edge).capacity -= path_flow;
                self.residual_graph.data_mut(rev_edge).capacity += path_flow;
            });
        }

        self.finished = true;
    }

    pub fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }
        Ok(self.max_flow)
    }

    pub fn assignment(&self, sources: &[NodeID]) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }

        // run a reachability analysis
        let mut reachable: BitVec = BitVec::with_capacity(self.residual_graph.number_of_nodes());
        reachable.resize(self.residual_graph.number_of_nodes(), false);
        let mut stack = Vec::new();
        for s in sources {
            stack.push(*s);
        }
        while let Some(node) = stack.pop() {
            if *reachable.get(node as usize).unwrap() {
                continue;
            }
            reachable.set(node as usize, true);
            // println!("reached {}", node);
            for edge in self.residual_graph.edge_range(node) {
                let target = self.residual_graph.target(edge);
                let reached = reachable.get(target as usize).unwrap();
                if !reached && self.residual_graph.data(edge).capacity > 0 {
                    stack.push(self.residual_graph.target(edge));
                }
            }
        }

        // retrieve min-cut by walking the graph
        // todo(dl): expose as interface
        for s in 0..self.residual_graph.number_of_nodes() as NodeID {
            for e in self.residual_graph.edge_range(s) {
                let t = self.residual_graph.target(e);
                if reachable.get(s as usize).unwrap() != reachable.get(t as usize).unwrap() {
                    // println!("cut edge ({},{})", s, t);
                }
            }
        }

        // println!("done.");
        Ok(reachable)
    }
}

#[cfg(test)]
mod tests {

    use crate::edge::InputEdge;
    use crate::ford_fulkerson::EdgeCapacity;
    use crate::ford_fulkerson::FordFulkerson;
    use bitvec::bits;
    use bitvec::prelude::Lsb0;

    #[test]
    fn max_flow_clr() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(16)),
            InputEdge::new(0, 2, EdgeCapacity::new(13)),
            InputEdge::new(1, 2, EdgeCapacity::new(10)),
            InputEdge::new(1, 3, EdgeCapacity::new(12)),
            InputEdge::new(2, 1, EdgeCapacity::new(4)),
            InputEdge::new(2, 4, EdgeCapacity::new(14)),
            InputEdge::new(3, 2, EdgeCapacity::new(9)),
            InputEdge::new(3, 5, EdgeCapacity::new(20)),
            InputEdge::new(4, 3, EdgeCapacity::new(7)),
            InputEdge::new(4, 5, EdgeCapacity::new(4)),
        ];

        let mut max_flow_solver = FordFulkerson::from_edge_list(edges);
        let sources = [0];
        let targets = [5];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(23, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");

        assert_eq!(assignment, bits![1, 1, 1, 0, 1, 0]);
    }

    #[test]
    fn max_flow_ita() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(5)),
            InputEdge::new(0, 4, EdgeCapacity::new(7)),
            InputEdge::new(0, 5, EdgeCapacity::new(6)),
            InputEdge::new(1, 2, EdgeCapacity::new(4)),
            InputEdge::new(1, 7, EdgeCapacity::new(3)),
            InputEdge::new(4, 7, EdgeCapacity::new(4)),
            InputEdge::new(4, 6, EdgeCapacity::new(1)),
            InputEdge::new(5, 6, EdgeCapacity::new(5)),
            InputEdge::new(2, 3, EdgeCapacity::new(3)),
            InputEdge::new(7, 3, EdgeCapacity::new(7)),
            InputEdge::new(6, 7, EdgeCapacity::new(1)),
            InputEdge::new(6, 3, EdgeCapacity::new(6)),
        ];

        let mut max_flow_solver = FordFulkerson::from_edge_list(edges);
        let sources = [0];
        let targets = [3];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(15, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 0, 0, 0, 1, 1, 0, 0]);
    }

    #[test]
    fn max_flow_yt() {
        let edges = vec![
            InputEdge::new(9, 0, EdgeCapacity::new(5)),
            InputEdge::new(9, 1, EdgeCapacity::new(10)),
            InputEdge::new(9, 2, EdgeCapacity::new(15)),
            InputEdge::new(0, 3, EdgeCapacity::new(10)),
            InputEdge::new(1, 0, EdgeCapacity::new(15)),
            InputEdge::new(1, 4, EdgeCapacity::new(20)),
            InputEdge::new(2, 5, EdgeCapacity::new(25)),
            InputEdge::new(3, 4, EdgeCapacity::new(25)),
            InputEdge::new(3, 6, EdgeCapacity::new(10)),
            InputEdge::new(4, 2, EdgeCapacity::new(5)),
            InputEdge::new(4, 7, EdgeCapacity::new(30)),
            InputEdge::new(5, 7, EdgeCapacity::new(20)),
            InputEdge::new(5, 8, EdgeCapacity::new(10)),
            InputEdge::new(7, 8, EdgeCapacity::new(15)),
            InputEdge::new(6, 10, EdgeCapacity::new(5)),
            InputEdge::new(7, 10, EdgeCapacity::new(15)),
            InputEdge::new(8, 10, EdgeCapacity::new(10)),
        ];

        let mut max_flow_solver = FordFulkerson::from_edge_list(edges);
        let sources = [9];
        let targets = [10];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(30, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn max_flow_ff() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        let mut max_flow_solver = FordFulkerson::from_edge_list(edges);
        let sources = [0];
        let targets = [5];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(9, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 1, 0, 1, 0, 0]);
    }

    #[test]
    #[should_panic]
    fn flow_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        FordFulkerson::from_edge_list(edges)
            .max_flow()
            .expect("max flow computation did not run");
    }

    #[test]
    #[should_panic]
    fn assignment_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        FordFulkerson::from_edge_list(edges)
            .assignment(&[0])
            .expect("assignment computation did not run");
    }
}
