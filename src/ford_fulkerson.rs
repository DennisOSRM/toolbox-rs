use crate::bfs::BFS;
use crate::graph::{Graph, NodeID};
use crate::static_graph::InputEdge;
use crate::static_graph::StaticGraph;
use bitvec::vec::BitVec;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeData {
    pub capacity: i32,
}

impl EdgeData {
    pub fn new(capacity: i32) -> EdgeData {
        EdgeData { capacity }
    }
}

pub struct FordFulkerson {
    residual_graph: StaticGraph<EdgeData>,
    finished: bool,
}

impl FordFulkerson {
    pub fn from_edge_list(mut edge_list: Vec<InputEdge<EdgeData>>) -> Self {
        let number_of_edges = edge_list.len();

        // blindly generate reverse edges for all edges
        edge_list.extend_from_within(..);
        edge_list.iter_mut().skip(number_of_edges).for_each(|edge| {
            edge.reverse();
            edge.data.capacity = 0;
        });
        // dedup-merge edge set
        edge_list.sort();
        edge_list.dedup_by(|a, mut b| {
            // merge duplicate edges by accumulating edge capacities
            let result = a.source == b.source && a.target == b.target;
            if result {
                b.data.capacity += a.data.capacity;
            }
            result
        });
        // at this point the edge set doesn't have any duplicates anymore.
        // note that this is fine, as we are looking to compute a node partition

        Self {
            residual_graph: StaticGraph::new(edge_list),
            finished: false,
        }
    }

    pub fn run(&mut self, sources: &[NodeID], targets: &[NodeID]) {
        let mut bfs = BFS::new();
        let filter = |graph: &StaticGraph<EdgeData>, edge| graph.data(edge).capacity <= 0;
        while bfs.run_with_filter(&self.residual_graph, sources, targets, filter) {
            // retrieve node path. This is sufficient, as we removed all duplicate edges
            let path = bfs.fetch_node_path();
            println!("found node path {:#?}", path);

            // find min capacity on edges of the path
            let st_tuple = path
                .windows(2)
                .min_by_key(|window| {
                    let edge = self.residual_graph.find_edge(window[0], window[1]).unwrap();
                    self.residual_graph.data(edge).capacity
                })
                .unwrap();

            let fwd_least_capacity_edge = self
                .residual_graph
                .find_edge(st_tuple[0], st_tuple[1])
                .unwrap();
            let path_flow = self.residual_graph.data(fwd_least_capacity_edge).capacity;
            assert!(path_flow > 0);
            println!(
                "min edge: {}, capacity: {}",
                fwd_least_capacity_edge, path_flow
            );

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

    pub fn retrieve_assignment(&self, sources: &[NodeID]) -> Result<BitVec, String> {
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
            reachable.set(node as usize, true);
            println!("reached {}", node);
            for edge in self.residual_graph.edge_range(node) {
                let target = self.residual_graph.target(edge);
                let reached = reachable.get(target as usize).unwrap();
                if !reached && self.residual_graph.data(edge).capacity > 0 {
                    stack.push(self.residual_graph.target(edge));
                }
            }
        }

        // todo(dluxen): retrieve min-cut
        // iterate all edges, output those with negative flow
        for s in 0..self.residual_graph.number_of_nodes() as NodeID {
            for e in self.residual_graph.edge_range(s) {
                let t = self.residual_graph.target(e);
                if reachable.get(s as usize).unwrap() != reachable.get(t as usize).unwrap() {
                    println!("cut edge ({},{})", s, t);
                }
            }
        }

        println!("done.");
        Ok(reachable)
    }
}

// 0,1,16
// 0,2,13
// 1,2,10
// 1,3,12
// 2,1,4
// 2,4,14
// 3,2,9
// 3,5,20
// 4,3,7,
// 4,5,4

#[cfg(test)]
mod tests {

    use crate::ford_fulkerson::EdgeData;
    use crate::ford_fulkerson::FordFulkerson;
    use crate::ford_fulkerson::InputEdge;

    #[test]
    fn max_flow_clr() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeData::new(16)),
            InputEdge::new(0, 2, EdgeData::new(13)),
            InputEdge::new(1, 2, EdgeData::new(10)),
            InputEdge::new(1, 3, EdgeData::new(12)),
            InputEdge::new(2, 1, EdgeData::new(4)),
            InputEdge::new(2, 4, EdgeData::new(14)),
            InputEdge::new(3, 2, EdgeData::new(9)),
            InputEdge::new(3, 5, EdgeData::new(20)),
            InputEdge::new(4, 3, EdgeData::new(7)),
            InputEdge::new(4, 5, EdgeData::new(4)),
        ];

        let mut max_flow = FordFulkerson::from_edge_list(edges);
        let sources = [0];
        let targets = [5];
        max_flow.run(&sources, &targets);

        // todo(dluxen): retrieve max-flow, requires an indicator if an edge
        // was an input edge or is "just" a residual graph edge

        let assigment = max_flow.retrieve_assignment(&sources);
        assert!(assigment.is_ok());
    }
}
