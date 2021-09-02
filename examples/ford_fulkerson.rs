use staticgraph::{bfs::BFS, graph::Graph, static_graph::*};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeData {
    pub capacity: i32,
}

impl EdgeData {
    pub fn new(capacity: i32) -> EdgeData {
        EdgeData { capacity }
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

fn main() {
    type Graph = StaticGraph<EdgeData>;
    let mut edges = vec![
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
    let number_of_edges = edges.len();

    // blindly generate reverse edges for all edges
    edges.extend_from_within(..);
    edges.iter_mut().skip(number_of_edges).for_each(|edge| {
        edge.reverse();
        edge.data.capacity = 0;
    });
    // dedup-merge edge set
    edges.sort();
    edges.dedup_by(|a, mut b| {
        // merge duplicate edges by accumulating edge capacities
        let result = a.source == b.source && a.target == b.target;
        if result {
            b.data.capacity += a.data.capacity;
        }
        result
    });
    // at this point the edge set doesn't have any duplicates anymore.
    // note that this is fine, as we are looking to compute a node partition

    // println!("{:#?}", edges);
    // println!(
    //     "edge list len: {}, capacity: {}",
    //     edges.len(),
    //     edges.capacity()
    // );

    let mut graph = Graph::new(edges);
    let mut bfs = BFS::new();
    let filter = |graph: &Graph, edge| graph.data(edge).capacity <= 0;
    while bfs.run_with_filter(&graph, vec![0], vec![5], filter) {
        // retrieve node path. This is sufficient, as we removed all duplicate edges
        let path = bfs.fetch_node_path();
        println!("found node path {:#?}", path);

        // find min capacity on edges of the path
        let st_tuple = path
            .windows(2)
            .min_by_key(|window| {
                let edge = graph.find_edge(window[0], window[1]).unwrap();
                graph.data(edge).capacity
            })
            .unwrap();

        let fwd_least_capacity_edge = graph.find_edge(st_tuple[0], st_tuple[1]).unwrap();
        let path_flow = graph.data(fwd_least_capacity_edge).capacity;
        assert!(path_flow > 0);
        println!(
            "min edge: {}, capacity: {}",
            fwd_least_capacity_edge, path_flow
        );

        // assign flow to residual graph
        path.windows(2).for_each(|pair| {
            let fwd_edge = graph.find_edge(pair[0], pair[1]).unwrap();
            let rev_edge = graph.find_edge(pair[1], pair[0]).unwrap();

            graph.data_mut(fwd_edge).capacity -= path_flow;
            graph.data_mut(rev_edge).capacity += path_flow;
        });
    }

    // todo(dluxen): retrieve max-flow
    // todo(dluxen): retrieve min-cut

    println!("done.")
}
