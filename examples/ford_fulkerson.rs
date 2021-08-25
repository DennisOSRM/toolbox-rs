use staticgraph::{bfs::BFS, static_graph::*};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeData {
    pub capacity: i32,
    pub forward: bool,
}

impl EdgeData {
    pub fn new(capacity: i32, forward: bool) -> EdgeData {
        EdgeData { capacity, forward }
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
        InputEdge::new(0, 1, EdgeData::new(16, true)),
        InputEdge::new(0, 2, EdgeData::new(13, true)),
        InputEdge::new(1, 2, EdgeData::new(10, true)),
        InputEdge::new(1, 3, EdgeData::new(12, true)),
        InputEdge::new(2, 1, EdgeData::new(4, true)),
        InputEdge::new(2, 4, EdgeData::new(14, true)),
        InputEdge::new(3, 2, EdgeData::new(9, true)),
        InputEdge::new(3, 5, EdgeData::new(20, true)),
        InputEdge::new(4, 3, EdgeData::new(7, true)),
        InputEdge::new(4, 5, EdgeData::new(4, true)),
    ];
    let number_of_edges = edges.len();

    // duplicate edge set
    edges.extend_from_within(..);
    // flag new edges as residual graphs edges
    edges.iter_mut().skip(number_of_edges).for_each(|edge| {
        edge.reverse();
        edge.data.forward = false;
    });
    // dedup-merge edge set
    edges.sort();
    edges.dedup_by(|a, mut b| {
        let result = a.source == b.source && a.target == b.target;
        if result {
            b.data.forward = true;
        }
        result
    });

    println!("{:#?}", edges);
    println!("len: {}, capacity: {}", edges.len(), edges.capacity());

    let mut graph = Graph::new(edges);
    let mut bfs = BFS::new(&graph);
    while bfs.run(vec![0], vec![5]) {
        // retrieve path
        // todo(dluxen): switch to edge path
        let path = bfs.fetch_node_path(5);
        assert_eq!(path, vec![0, 1, 3, 5]);
        println!("found path {:#?}", path);

        // todo(dluxen): assign flow to Graph
    }

    // todo(dluxen): retrieve cut
}
