use crate::graph::NodeID;

#[derive(Clone, Debug, Default)]
pub struct Cell {
    border_nodes: Vec<NodeID>,
    distance_matrix: Vec<usize>,
    id: usize,
}

impl Cell {
    pub fn new(border_nodes: Vec<NodeID>, distance_matrix: Vec<usize>, id: usize) -> Self {
        Self {
            border_nodes,
            distance_matrix,
            id
        }
    }

    pub fn get_distance(&self, source: usize, target: usize) -> usize {
        self.distance_matrix[source * self.border_nodes.len() + target]
    }

    pub fn id(&self) -> usize {
        self.id
    }
}