pub mod binary_heap;
pub mod static_graph;
pub mod addressable_binary_heap;

#[cfg(test)]
mod tests {
    use crate::static_graph::{InputEdge, StaticGraph};

    #[test]
    fn size_of_graph() {
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

        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());
    }

    #[test]
    fn static_graph_degree() {
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
        let mut sum = 0;
        for i in graph.node_range() {
            sum += graph.get_out_degree(i);
        }
        assert_eq!(sum, graph.number_of_edges());
    }

    #[test]
    fn cycle_check_no_cycle() {
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
        assert_eq!(false, graph.cycle_check());
    }

    #[test]
    fn cycle_check_cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(2, 3, 3),
            InputEdge::new(3, 4, 1),
            InputEdge::new(4, 5, 6),
            InputEdge::new(5, 2, 2),
        ];
        let graph = Graph::new(edges);
        assert_eq!(true, graph.cycle_check());
    }


}
