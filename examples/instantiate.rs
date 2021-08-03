use staticgraph::static_graph::*;

fn main() {
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

    println!("number of nodes: {}", graph.number_of_nodes());
    println!("number of edges: {}", graph.number_of_edges());

    for i in graph.node_range() {
        println!("out_degree({})={}", i, graph.get_out_degree(i));
        for j in graph.begin_edges(i)..graph.end_edges(i) {
            println!(" ({},{}): {}", i, graph.target(j), graph.data(j));
        }
    }
}
