use crate::{addressable_binary_heap::AddressableHeap, graph::NodeID};

pub struct SearchSpace<'a> {
    queue: &'a AddressableHeap<NodeID, usize, NodeID>,
}

impl<'a> SearchSpace<'a> {
    pub fn new(queue: &'a AddressableHeap<NodeID, usize, NodeID>) -> Self {
        Self { queue }
    }

    pub fn get_parent(&self, node: NodeID) -> Option<NodeID> {
        if !self.queue.inserted(node) {
            return None;
        }
        Some(*self.queue.data(node))
    }
}

#[cfg(test)]
mod tests {

    use crate::{
        edge::{InputEdge, SimpleEdge},
        graph::Graph,
        static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    fn create_graph() -> StaticGraph<usize> {
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
        let graph = StaticGraph::<usize>::new(edges);
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());

        graph
    }

    #[test]
    fn dfs_on_search_space_from_dijkstra() {
        let graph = create_graph();
        let mut dijkstra = UnidirectionalDijkstra::new();
        let source = 0;
        let target = 3;
        let distance = dijkstra.run(&graph, source, target);
        assert_eq!(6, dijkstra.search_space_len());
        assert_eq!(9, distance);

        let search_space = dijkstra.search_space();

        // retrieve edges

        let edges: Vec<_> = graph
            .node_range()
            // not all nodes may have a parent
            .filter(|node| search_space.get_parent(*node).is_some() || *node != source)
            .map(|node| {
                let parent = search_space.get_parent(node).unwrap();

                println!("({parent},{node})");
                SimpleEdge {
                    source: parent,
                    target: node,
                    data: 1,
                }
            })
            .collect();

        // construct DAG
        let graph = StaticGraph::new(edges);
        println!(
            "nodes: {}, edges: {}",
            graph.number_of_nodes(),
            graph.number_of_edges()
        );

        // walk DAG in a DFS
        let mut visited = vec![false; graph.number_of_nodes()];

        let mut stack = vec![(source, true)];
        visited[source] = true;
        while let Some((_current, pre_traversal)) = stack.pop() {
            if pre_traversal {
            } else {
            }
        }
    }
}
