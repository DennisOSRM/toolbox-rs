use std::{
    cmp::max,
    ops::Index,
    sync::{atomic::AtomicI32, Arc},
};

use bitvec::prelude::BitVec;
use log::{debug, info};

use crate::{
    dinic::Dinic,
    edge::InputEdge,
    geometry::primitives::FPCoordinate,
    max_flow::{MaxFlow, ResidualCapacity},
};

pub struct Coefficients([(i32, i32); 4]);
// coefficients for rotation matrix at 0, 90, 180 and 270 degrees

impl Default for Coefficients {
    fn default() -> Self {
        Self::new()
    }
}

impl Coefficients {
    pub fn new() -> Self {
        Coefficients([(0, 1), (1, 0), (1, 1), (-1, 1)])
    }
}

impl Index<usize> for Coefficients {
    type Output = (i32, i32);
    fn index(&self, i: usize) -> &(i32, i32) {
        &self.0[i % self.0.len()]
    }
}

/// Computes the inertial flow cut for a given orientation and balance
///
/// # Arguments
///
/// * `index` - which of the (0..4) substeps to execute
/// * `edges` - a list of edges that represents the input graph
/// * `node_id_list` - list of node ids
/// * `coordinates` - immutable slice of coordinates of the graphs nodes
/// * `balance_factor` - balance factor, i.e. how many nodes get contracted
/// * `upper_bound` - a global upperbound to the best inertial flow cut
pub fn sub_step(
    index: usize,
    input_edges: &[InputEdge<ResidualCapacity>],
    coordinates: &[FPCoordinate],
    node_id_list: &[usize],
    balance_factor: f64,
    upper_bound: Arc<AtomicI32>,
) -> (i32, f64, bitvec::vec::BitVec, Vec<usize>) {
    debug_assert!(index < 4);
    debug_assert!(balance_factor > 0.);
    debug_assert!(balance_factor < 0.5);
    debug_assert!(coordinates.len() > 2);

    let current_coefficients = &Coefficients::new()[index];
    info!("[{index}] sorting cooefficient: {:?}", current_coefficients);
    // the iteration proxy list to be sorted. The coordinates vector itself is not touched.
    let mut node_id_list = node_id_list.to_vec();
    node_id_list.sort_unstable_by_key(|a| -> i32 {
        coordinates[*a].lon * current_coefficients.0 + coordinates[*a].lat * current_coefficients.1
    });

    let size_of_contraction = max(1, (node_id_list.len() as f64 * balance_factor) as usize);
    let sources = &node_id_list[0..size_of_contraction as usize];
    let targets = &node_id_list[node_id_list.len() - (size_of_contraction as usize)..];

    debug_assert!(!sources.is_empty());
    debug_assert!(!targets.is_empty());

    info!("[{index}] renumbering of inertial flow graph");
    let mut renumbering_table = vec![usize::MAX; coordinates.len()];
    // nodes in the in the graph have to be numbered consecutively.
    // the mapping is input id -> dinic id

    for s in sources {
        renumbering_table[*s] = 0;
    }
    for t in targets {
        renumbering_table[*t] = 1;
    }

    // each thread holds their own copy of the edge set
    let mut edges = Vec::new();
    edges.extend_from_slice(input_edges);
    let mut current_id = 2;

    for mut e in &mut edges {
        // nodes in the in the graph have to be numbered consecutively
        if renumbering_table[e.source] == usize::MAX {
            renumbering_table[e.source] = current_id;
            current_id += 1;
        }
        if renumbering_table[e.target] == usize::MAX {
            renumbering_table[e.target] = current_id;
            current_id += 1;
        }
        e.source = renumbering_table[e.source];
        e.target = renumbering_table[e.target];
    }
    info!("[{index}] instantiating min-cut solver, epsilon 0.25");

    // remove eigenloops especially from contracted regions
    let edge_count_before = edges.len();
    edges.retain(|edge| edge.source != edge.target);
    info!(
        "[{index}] eigenloop removal - edge count before {edge_count_before}, after {}",
        edges.len()
    );
    edges.shrink_to_fit();

    info!("[{index}] instantiating min-cut solver, epsilon {balance_factor}");
    let mut max_flow_solver = Dinic::from_edge_list(edges, 0, 1);
    info!("[{index}] instantiated min-cut solver");
    max_flow_solver.run_with_upper_bound(upper_bound);

    let max_flow = max_flow_solver.max_flow();

    if max_flow.is_err() {
        // Error is returned in case the search is aborted early
        return (i32::MAX, 0., BitVec::new(), Vec::new());
    }
    let max_flow = max_flow.expect("max flow computation did not run");

    info!("[{index}] computed max flow: {max_flow}");
    let assignment = max_flow_solver
        .assignment(0)
        .expect("max flow computation did not run");

    let left_size = assignment.iter().filter(|b| !**b).count() + sources.len() - 1;
    let right_size = assignment.iter().filter(|b| **b).count() + targets.len() - 1;
    info!(
        "[{index}] assignment has {} total entries",
        left_size + right_size
    );
    debug!("[{index}] assignment has {right_size} 1-entries");
    debug!("[{index}] assignment has {left_size} 0-entries");

    let balance = std::cmp::min(left_size, right_size) as f64 / (right_size + left_size) as f64;
    info!("[{index}] balance: {balance}");

    (max_flow, balance, assignment, renumbering_table)
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicI32, Arc};

    use bitvec::bits;
    use bitvec::prelude::*;
    use itertools::Itertools;

    use crate::{
        edge::InputEdge, geometry::primitives::FPCoordinate, inertial_flow::sub_step,
        max_flow::ResidualCapacity,
    };

    use super::Coefficients;

    #[test]
    fn iterate_with_wrap() {
        let coefficients = Coefficients::new();

        (0..4).zip(4..8).for_each(|index| {
            assert_eq!(coefficients[index.0], coefficients[index.1]);
        });
    }

    #[test]
    fn inertial_flow() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(1)),
            InputEdge::new(1, 0, ResidualCapacity::new(1)),
            InputEdge::new(0, 2, ResidualCapacity::new(1)),
            InputEdge::new(2, 0, ResidualCapacity::new(1)),
            InputEdge::new(1, 2, ResidualCapacity::new(1)),
            InputEdge::new(2, 1, ResidualCapacity::new(1)),
            InputEdge::new(2, 4, ResidualCapacity::new(1)),
            InputEdge::new(4, 2, ResidualCapacity::new(1)),
            InputEdge::new(3, 5, ResidualCapacity::new(1)),
            InputEdge::new(5, 3, ResidualCapacity::new(1)),
            InputEdge::new(4, 3, ResidualCapacity::new(1)),
            InputEdge::new(3, 4, ResidualCapacity::new(1)),
            InputEdge::new(4, 5, ResidualCapacity::new(1)),
            InputEdge::new(5, 4, ResidualCapacity::new(1)),
        ];

        let upper_bound = Arc::new(AtomicI32::new(6));

        let coordinates = vec![
            FPCoordinate::new(1, 0),
            FPCoordinate::new(2, 1),
            FPCoordinate::new(0, 1),
            FPCoordinate::new(2, 2),
            FPCoordinate::new(0, 2),
            FPCoordinate::new(1, 3),
        ];
        let node_id_list = (0..coordinates.len()).collect_vec();

        let (flow, balance, assignment, _table) =
            sub_step(3, &edges, &coordinates, &node_id_list, 0.25, upper_bound);
        assert_eq!(flow, 1);
        assert_eq!(balance, 0.5);
        assert_eq!(assignment.len(), 6);
        assert_eq!(assignment, bits![1, 0, 0, 0, 1, 1]);
    }
}
