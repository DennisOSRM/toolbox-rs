use fxhash::FxHashMap;
use itertools::Itertools;
use log::debug;

use crate::{
    edge::InputEdge, graph::NodeID, one_to_many_dijkstra::OneToManyDijkstra,
    static_graph::StaticGraph,
};

#[derive(Clone, Debug)]
pub struct BaseCell {
    // TODO: experiment on thin_vec
    pub incoming_nodes: Vec<NodeID>,
    pub outgoing_nodes: Vec<NodeID>,
    pub edges: Vec<InputEdge<usize>>,
    // TODO: add renumbering table to support unpacking edges
}

impl Default for BaseCell {
    fn default() -> Self {
        Self::new()
    }
}

impl BaseCell {
    pub fn new() -> Self {
        BaseCell {
            incoming_nodes: Vec::new(),
            outgoing_nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn process(&self) -> MatrixCell {
        // renumber nodes to be in contiguous range"
        // [sources..targets..other nodes]
        // [0,1,2         ...         n-1]

        // println!("processing cell {:?}", self);
        let mut seen_nodes = FxHashMap::default();
        self.incoming_nodes.iter().for_each(|node| {
            let next_id = seen_nodes.len();
            seen_nodes.entry(*node).or_insert(next_id);
            // println!("source {}, idx: {}", *node, seen_nodes.len());
        });
        // println!("1");
        self.outgoing_nodes.iter().for_each(|node| {
            let next_id = seen_nodes.len();
            seen_nodes.entry(*node).or_insert(next_id);
            // println!("target {}, idx: {}", *node, seen_nodes.len());
        });
        // println!("2");

        let new_edges = self
            .edges
            .iter()
            .map(|edge| {
                // renumber source/target nodes of edges to be in range [0..k-1]
                let mut new_edge = *edge;

                let seen_nodes_len = seen_nodes.len();
                seen_nodes.entry(edge.source).or_insert(seen_nodes_len);
                new_edge.source = *seen_nodes.get(&edge.source).expect("renumbering broken");

                let seen_nodes_len = seen_nodes.len();
                seen_nodes.entry(edge.target).or_insert(seen_nodes_len);
                new_edge.target = *seen_nodes.get(&edge.target).expect("renumbering broken");

                new_edge
            })
            .collect_vec();

        // instantiate subgraph
        let graph = StaticGraph::new(new_edges);
        let mut dijkstra = OneToManyDijkstra::new();
        let mut matrix = vec![usize::MAX; self.incoming_nodes.len() * self.outgoing_nodes.len()];

        let source_range = 0..self.incoming_nodes.len();
        let target_range = (self.incoming_nodes.len()
            ..self.incoming_nodes.len() + self.outgoing_nodes.len())
            .into_iter()
            .collect_vec();
        // println!("3, graph: ({},{})", graph.number_of_nodes(), graph.number_of_edges());
        if !self.edges.is_empty() {
            for source in source_range {
                // compute clique information repeated one-to-many calls for each source
                let _success = dijkstra.run(&graph, source, &target_range);
                for target in &target_range {
                    let distance = dijkstra.distance(*target);
                    let target_index = target - self.incoming_nodes.len();
                    debug!(
                        "matrix[{}] distance({source},{target})={distance}",
                        (source * self.outgoing_nodes.len() + target_index)
                    );

                    matrix[source * self.outgoing_nodes.len() + target_index] = distance;
                }
            }
        }
        // println!("4");

        // return MatrixCell
        MatrixCell {
            matrix,
            incoming_nodes: self.incoming_nodes.clone(),
            outgoing_nodes: self.outgoing_nodes.clone(),
        }
    }
}

#[derive(Clone)]
pub struct MatrixCell {
    // TODO: experiment on thin_vec
    pub incoming_nodes: Vec<NodeID>,
    pub outgoing_nodes: Vec<NodeID>,
    // matrix of pairwise distances between boundary nodes
    pub matrix: Vec<usize>,
}

impl MatrixCell {
    // TODO: iterator for row and column access
    pub fn get_distance_row(&self, u: NodeID) -> &[usize] {
        // get row in matrix for node u
        let index = self
            .incoming_nodes
            .binary_search(&u)
            .unwrap_or_else(|_| panic!("node {u} not found in node boundary"));
        // return the row of the matrix
        &self.matrix[index * self.incoming_nodes.len()..(index + 1) * self.outgoing_nodes.len()]
    }

    pub fn overlay_edges(&self) -> Vec<InputEdge<usize>> {
        let mut result = Vec::new();
        result.reserve(self.incoming_nodes.len() * self.outgoing_nodes.len());

        // walk matrix to derive list of edges for the next level of processing
        for i in 0..self.incoming_nodes.len() {
            let source = self.incoming_nodes[i];
            for j in 0..self.outgoing_nodes.len() {
                let distance = self.matrix[i * j + i];
                if distance != usize::MAX {
                    let target = self.outgoing_nodes[j];
                    let edge = InputEdge {
                        source,
                        target,
                        data: distance,
                    };
                    // edge_count += 1;
                    result.push(edge);
                }
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::BaseCell;
    use crate::{edge::InputEdge, graph::UNREACHABLE};

    #[test]
    fn process_base_cell1() {
        let edges: Vec<InputEdge<usize>> = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];

        // check first set of source, target nodes
        let incoming_nodes = vec![0, 4];
        let outgoing_nodes = vec![3, 5];

        let base_cell = BaseCell {
            incoming_nodes: incoming_nodes.clone(),
            outgoing_nodes: outgoing_nodes.clone(),
            edges: edges.clone(),
        };
        let matrix_cell = base_cell.process();

        assert_eq!(incoming_nodes, matrix_cell.incoming_nodes);
        assert_eq!(outgoing_nodes, matrix_cell.outgoing_nodes);
        assert_eq!(matrix_cell.matrix, vec![9, 4, 7, 2]);

        assert_eq!(matrix_cell.get_distance_row(0), vec![9, 4]);
        assert_eq!(matrix_cell.get_distance_row(4), vec![7, 2]);

        // check second set of source, target nodes
        let incoming_nodes = vec![0, 1];
        let outgoing_nodes = vec![4, 5];

        let base_cell = BaseCell {
            incoming_nodes: incoming_nodes.clone(),
            outgoing_nodes: outgoing_nodes.clone(),
            edges,
        };
        let matrix_cell = base_cell.process();

        assert_eq!(incoming_nodes, matrix_cell.incoming_nodes);
        assert_eq!(outgoing_nodes, matrix_cell.outgoing_nodes);
        assert_eq!(matrix_cell.matrix, vec![2, 4, UNREACHABLE, 2]);

        assert_eq!(matrix_cell.get_distance_row(0), vec![2, 4]);
        assert_eq!(matrix_cell.get_distance_row(1), vec![UNREACHABLE, 2]);
    }

    #[test]
    fn process_base_cell2() {
        let edges = vec![
            InputEdge::new(0, 1, 7),
            InputEdge::new(0, 2, 3),
            InputEdge::new(1, 2, 1),
            InputEdge::new(1, 3, 6),
            InputEdge::new(2, 4, 8),
            InputEdge::new(3, 5, 2),
            InputEdge::new(3, 2, 3),
            InputEdge::new(4, 3, 2),
            InputEdge::new(4, 5, 8),
        ];

        // check first set of source, target nodes
        let incoming_nodes = vec![0, 1];
        let outgoing_nodes = vec![4, 5];

        let base_cell = BaseCell {
            incoming_nodes: incoming_nodes.clone(),
            outgoing_nodes: outgoing_nodes.clone(),
            edges: edges.clone(),
        };
        let matrix_cell = base_cell.process();

        assert_eq!(incoming_nodes, matrix_cell.incoming_nodes);
        assert_eq!(outgoing_nodes, matrix_cell.outgoing_nodes);
        assert_eq!(matrix_cell.matrix, vec![11, 15, 9, 8]);

        assert_eq!(matrix_cell.get_distance_row(0), vec![11, 15]);
        assert_eq!(matrix_cell.get_distance_row(1), vec![9, 8]);

        // check second set of source, target nodes
        let incoming_nodes = vec![0, 2];
        let outgoing_nodes = vec![3, 5];

        let base_cell = BaseCell {
            incoming_nodes: incoming_nodes.clone(),
            outgoing_nodes: outgoing_nodes.clone(),
            edges,
        };
        let matrix_cell = base_cell.process();

        assert_eq!(incoming_nodes, matrix_cell.incoming_nodes);
        assert_eq!(outgoing_nodes, matrix_cell.outgoing_nodes);
        assert_eq!(matrix_cell.matrix, vec![13, 15, 10, 12]);

        assert_eq!(matrix_cell.get_distance_row(0), vec![13, 15]);
        assert_eq!(matrix_cell.get_distance_row(2), vec![10, 12]);
    }

    #[test]
    #[should_panic]
    fn matrix_cell_row_invalid() {
        let edges: Vec<InputEdge<usize>> = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];

        // check first set of source, target nodes
        let incoming_nodes = vec![0, 4];
        let outgoing_nodes = vec![3, 5];

        let base_cell = BaseCell {
            incoming_nodes,
            outgoing_nodes,
            edges,
        };
        let matrix_cell = base_cell.process();
        matrix_cell.get_distance_row(1);
    }

    #[test]
    fn dimacs_extract() {
        // regression test from handling DIMACS data set
        let incoming_nodes = vec![
            9425886, 8380081, 9425867, 8380040, 8380040, 9425848, 8380040, 9425887, 9425899,
            9425952, 10105412, 10105432, 9425958, 8380092,
        ];
        let outgoing_nodes = vec![
            9425844, 9425847, 9425861, 8380080, 10105465, 9425852, 8380082, 8380066, 9425885,
            9425900, 9425953, 10105463, 10105408, 10105431,
        ];
        let edges = vec![
            InputEdge::new(8380040, 9425844, 2852),
            InputEdge::new(8380040, 9425847, 1641),
            InputEdge::new(8380040, 9425849, 1334),
            InputEdge::new(8380040, 9425861, 425),
            InputEdge::new(8380040, 9425866, 1380),
            InputEdge::new(8380051, 9425870, 2713),
            InputEdge::new(8380051, 9425891, 2378),
            InputEdge::new(8380051, 10105410, 1114),
            InputEdge::new(8380051, 10105412, 1013),
            InputEdge::new(8380052, 9425891, 1225),
            InputEdge::new(8380052, 9425893, 892),
            InputEdge::new(8380052, 10105410, 2375),
            InputEdge::new(8380053, 9425893, 2497),
            InputEdge::new(8380053, 9425921, 885),
            InputEdge::new(8380053, 10105410, 1332),
            InputEdge::new(8380073, 8380074, 2886),
            InputEdge::new(8380073, 8380075, 864),
            InputEdge::new(8380073, 9425896, 126),
            InputEdge::new(8380074, 8380073, 2886),
            InputEdge::new(8380075, 8380073, 864),
            InputEdge::new(8380075, 8380076, 3560),
            InputEdge::new(8380075, 8380078, 1770),
            InputEdge::new(8380075, 9425897, 826),
            InputEdge::new(8380076, 8380075, 3560),
            InputEdge::new(8380076, 9425896, 3335),
            InputEdge::new(8380076, 9425956, 2295),
            InputEdge::new(8380078, 8380075, 1770),
            InputEdge::new(8380081, 8380080, 667),
            InputEdge::new(8380081, 8380083, 901),
            InputEdge::new(8380081, 10105432, 1233),
            InputEdge::new(8380083, 8380081, 901),
            InputEdge::new(8380088, 8380089, 1638),
            InputEdge::new(8380088, 8380090, 889),
            InputEdge::new(8380088, 9425950, 2582),
            InputEdge::new(8380089, 8380088, 1638),
            InputEdge::new(8380090, 8380088, 889),
            InputEdge::new(8380090, 8380091, 1311),
            InputEdge::new(8380090, 8380092, 508),
            InputEdge::new(8380091, 8380090, 1311),
            InputEdge::new(8380092, 8380090, 508),
            InputEdge::new(8380092, 9425952, 3106),
            InputEdge::new(8380092, 10105464, 1979),
            InputEdge::new(8380092, 10105465, 1334),
            InputEdge::new(9425848, 9425849, 1917),
            InputEdge::new(9425848, 9425850, 859),
            InputEdge::new(9425848, 9425852, 1140),
            InputEdge::new(9425848, 9425867, 2888),
            InputEdge::new(9425848, 9425868, 1885),
            InputEdge::new(9425849, 8380040, 1334),
            InputEdge::new(9425849, 9425848, 1917),
            InputEdge::new(9425849, 9425850, 1657),
            InputEdge::new(9425850, 9425848, 859),
            InputEdge::new(9425850, 9425849, 1657),
            InputEdge::new(9425850, 9425869, 1253),
            InputEdge::new(9425850, 10105411, 2474),
            InputEdge::new(9425866, 8380040, 1380),
            InputEdge::new(9425866, 9425869, 690),
            InputEdge::new(9425866, 10105412, 3284),
            InputEdge::new(9425867, 8380082, 1249),
            InputEdge::new(9425867, 9425848, 2888),
            InputEdge::new(9425867, 9425919, 1560),
            InputEdge::new(9425868, 9425848, 1885),
            InputEdge::new(9425868, 9425919, 1525),
            InputEdge::new(9425868, 9425920, 2467),
            InputEdge::new(9425869, 9425850, 1253),
            InputEdge::new(9425869, 9425866, 690),
            InputEdge::new(9425869, 9425870, 552),
            InputEdge::new(9425870, 8380051, 2713),
            InputEdge::new(9425870, 9425869, 552),
            InputEdge::new(9425870, 10105406, 1196),
            InputEdge::new(9425886, 8380066, 2224),
            InputEdge::new(9425886, 9425887, 584),
            InputEdge::new(9425886, 9425889, 2113),
            InputEdge::new(9425886, 9425890, 1065),
            InputEdge::new(9425887, 9425885, 491),
            InputEdge::new(9425887, 9425886, 584),
            InputEdge::new(9425887, 9425888, 904),
            InputEdge::new(9425888, 9425887, 904),
            InputEdge::new(9425888, 9425891, 1111),
            InputEdge::new(9425888, 10105412, 2549),
            InputEdge::new(9425889, 9425886, 2113),
            InputEdge::new(9425889, 9425891, 491),
            InputEdge::new(9425889, 9425892, 2112),
            InputEdge::new(9425890, 9425886, 1065),
            InputEdge::new(9425890, 9425894, 983),
            InputEdge::new(9425890, 9425895, 4556),
            InputEdge::new(9425891, 8380051, 2378),
            InputEdge::new(9425891, 8380052, 1225),
            InputEdge::new(9425891, 9425888, 1111),
            InputEdge::new(9425891, 9425889, 491),
            InputEdge::new(9425892, 9425889, 2112),
            InputEdge::new(9425892, 9425893, 573),
            InputEdge::new(9425892, 9425895, 1038),
            InputEdge::new(9425892, 9425957, 3897),
            InputEdge::new(9425893, 8380052, 892),
            InputEdge::new(9425893, 8380053, 2497),
            InputEdge::new(9425893, 9425892, 573),
            InputEdge::new(9425894, 9425890, 983),
            InputEdge::new(9425894, 9425896, 1070),
            InputEdge::new(9425894, 9425954, 5245),
            InputEdge::new(9425895, 9425890, 4556),
            InputEdge::new(9425895, 9425892, 1038),
            InputEdge::new(9425895, 9425954, 1544),
            InputEdge::new(9425895, 9425955, 3563),
            InputEdge::new(9425896, 8380073, 126),
            InputEdge::new(9425896, 8380076, 3335),
            InputEdge::new(9425896, 9425894, 1070),
            InputEdge::new(9425897, 8380075, 826),
            InputEdge::new(9425897, 9425898, 672),
            InputEdge::new(9425897, 9425899, 989),
            InputEdge::new(9425898, 9425897, 672),
            InputEdge::new(9425899, 9425897, 989),
            InputEdge::new(9425899, 9425900, 424),
            InputEdge::new(9425919, 9425867, 1560),
            InputEdge::new(9425919, 9425868, 1525),
            InputEdge::new(9425919, 10105437, 2967),
            InputEdge::new(9425920, 9425868, 2467),
            InputEdge::new(9425920, 9425921, 414),
            InputEdge::new(9425920, 10105411, 1016),
            InputEdge::new(9425921, 8380053, 885),
            InputEdge::new(9425921, 9425920, 414),
            InputEdge::new(9425921, 10105437, 1242),
            InputEdge::new(9425950, 8380088, 2582),
            InputEdge::new(9425950, 9425951, 828),
            InputEdge::new(9425950, 9425957, 1589),
            InputEdge::new(9425950, 10105438, 1657),
            InputEdge::new(9425951, 9425950, 828),
            InputEdge::new(9425951, 9425952, 371),
            InputEdge::new(9425951, 10105461, 861),
            InputEdge::new(9425952, 8380092, 3106),
            InputEdge::new(9425952, 9425951, 371),
            InputEdge::new(9425952, 9425953, 742),
            InputEdge::new(9425954, 9425894, 5245),
            InputEdge::new(9425954, 9425895, 1544),
            InputEdge::new(9425954, 9425956, 1306),
            InputEdge::new(9425955, 9425895, 3563),
            InputEdge::new(9425955, 9425957, 1202),
            InputEdge::new(9425955, 9425958, 997),
            InputEdge::new(9425956, 8380076, 2295),
            InputEdge::new(9425956, 9425954, 1306),
            InputEdge::new(9425957, 9425892, 3897),
            InputEdge::new(9425957, 9425950, 1589),
            InputEdge::new(9425957, 9425955, 1202),
            InputEdge::new(9425957, 10105438, 1667),
            InputEdge::new(9425958, 9425955, 997),
            InputEdge::new(9425958, 10105462, 616),
            InputEdge::new(9425958, 10105463, 1463),
            InputEdge::new(10105406, 9425870, 1196),
            InputEdge::new(10105406, 10105410, 1970),
            InputEdge::new(10105406, 10105411, 508),
            InputEdge::new(10105410, 8380051, 1114),
            InputEdge::new(10105410, 8380052, 2375),
            InputEdge::new(10105410, 8380053, 1332),
            InputEdge::new(10105410, 10105406, 1970),
            InputEdge::new(10105411, 9425850, 2474),
            InputEdge::new(10105411, 9425920, 1016),
            InputEdge::new(10105411, 10105406, 508),
            InputEdge::new(10105412, 8380051, 1013),
            InputEdge::new(10105412, 9425866, 3284),
            InputEdge::new(10105412, 9425888, 2549),
            InputEdge::new(10105412, 10105408, 1003),
            InputEdge::new(10105432, 8380081, 1233),
            InputEdge::new(10105432, 10105431, 1229),
            InputEdge::new(10105432, 10105438, 7863),
            InputEdge::new(10105437, 9425919, 2967),
            InputEdge::new(10105437, 9425921, 1242),
            InputEdge::new(10105437, 10105438, 2667),
            InputEdge::new(10105438, 9425950, 1657),
            InputEdge::new(10105438, 9425957, 1667),
            InputEdge::new(10105438, 10105432, 7863),
            InputEdge::new(10105438, 10105437, 2667),
            InputEdge::new(10105461, 9425951, 861),
            InputEdge::new(10105462, 9425958, 616),
            InputEdge::new(10105464, 8380092, 1979),
        ];

        let base_cell = BaseCell {
            incoming_nodes,
            outgoing_nodes,
            edges,
        };
        let _matrix_cell = base_cell.process();
        // assert_eq!(matrix_cell.incoming_nodes, incoming_nodes);
    }
}
