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

    // duplicate edge set
    edges.extend_from_within(..);
    // flag new edges as residual graphs edges
    edges.iter_mut().skip(number_of_edges).for_each(|edge| {
        edge.reverse();
        edge.data.capacity = 0;
    });
    // dedup-merge edge set
    edges.sort();
    edges.dedup_by(|a, mut b| {
        let result = a.source == b.source && a.target == b.target;
        if result {
            b.data.capacity += a.data.capacity;
        }
        result
    });

    println!("{:#?}", edges);
    println!("len: {}, capacity: {}", edges.len(), edges.capacity());

    let mut graph = Graph::new(edges);
    let mut bfs = BFS::new();
    let filter = |graph: &Graph, edge| graph.data(edge).capacity <= 0;
    while bfs.run_with_filter(&graph, vec![0], vec![5], filter) {
        // retrieve path
        let path = bfs.fetch_node_path();
        assert_eq!(path, vec![0, 1, 3, 5]);
        println!("found node path {:#?}", path);
        let path = bfs.fetch_edge_path(&graph);
        println!("found edge path {:#?}", path);

        // find min capacity on edges of the path
        let least_capacity_edge = *path
            .iter()
            .min_by_key(|x| graph.data(**x).capacity)
            .unwrap();
        let current_flow = graph.data(least_capacity_edge).capacity;
        assert!(current_flow > 0);
        println!(
            "min edge: {}, capacity: {}",
            least_capacity_edge, current_flow
        );

        // todo(dluxen): assign flow to Graph
        path.iter().for_each(|e| {
            graph.data_mut(*e).capacity -= current_flow;
            // let reverse_edge = graph .find_edge(s, t)
        });
        // break;
    }

    // todo(dluxen): retrieve cut

    println!("done.")
}
