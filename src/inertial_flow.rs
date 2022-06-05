use std::{
    cmp::max,
    ops::Index,
    sync::{atomic::AtomicI32, Arc},
};

use bitvec::prelude::BitVec;
use itertools::Itertools;
use log::debug;

use crate::{
    dinic::Dinic,
    edge::{InputEdge, TrivialEdge},
    geometry::primitives::FPCoordinate,
    max_flow::{MaxFlow, ResidualCapacity},
};

pub struct RotatedComparators([fn(i32, i32) -> i32; 4]);
// comparator equivalent to rotation matrix at 0, 90, 180 and 270 degrees

impl Default for RotatedComparators {
    fn default() -> Self {
        Self::new()
    }
}

impl RotatedComparators {
    pub fn new() -> Self {
        // Comparator functions use the follwoing coefficients: (0, 1), (1, 0), (1, 1), (-1, 1)])
        RotatedComparators([
            |lat, _lon| -> i32 { lat },
            |_lat, lon| -> i32 { lon },
            |lat, lon| -> i32 { lon + lat },
            |lat, lon| -> i32 { -lon + lat },
        ])
    }
}

impl Index<usize> for RotatedComparators {
    type Output = fn(i32, i32) -> i32;
    fn index(&self, i: usize) -> &fn(i32, i32) -> i32 {
        &self.0[i % self.0.len()]
    }
}

pub struct FlowResult {
    pub flow: i32,
    pub balance: f64,
    pub assignment: bitvec::vec::BitVec,
}

pub fn flow_cmp(a: &FlowResult, b: &FlowResult) -> std::cmp::Ordering {
    if a.flow == b.flow {
        // note that a and b are inverted here on purpose:
        // balance is at most 0.5 and the closer the value the more balanced the partitions
        return b.balance.partial_cmp(&a.balance).unwrap();
    }
    a.flow.cmp(&b.flow)
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
    axis: usize,
    input_edges: &[TrivialEdge],
    coordinates: &[FPCoordinate],
    node_id_list: &[usize],
    balance_factor: f64,
    upper_bound: Arc<AtomicI32>,
) -> FlowResult {
    debug_assert!(axis < 4);
    debug_assert!(balance_factor > 0.);
    debug_assert!(balance_factor < 0.5);
    debug_assert!(coordinates.len() > 2);

    let comparator = &RotatedComparators::new()[axis];
    debug!("[{axis}] sorting cooefficient: {:?}", comparator);
    // the iteration proxy list to be sorted. The coordinates vector itself is not touched.
    let mut node_id_list = node_id_list.to_vec();
    node_id_list
        .sort_unstable_by_key(|a| -> i32 { comparator(coordinates[*a].lat, coordinates[*a].lon) });

    let size_of_contraction = max(1, (node_id_list.len() as f64 * balance_factor) as usize);
    let sources = &node_id_list[0..size_of_contraction as usize];
    let targets = &node_id_list[node_id_list.len() - (size_of_contraction as usize)..];

    debug_assert!(!sources.is_empty());
    debug_assert!(!targets.is_empty());

    debug!("[{axis}] renumbering of inertial flow graph");
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
    let mut edges = input_edges
        .iter()
        .map(|edge| -> InputEdge<ResidualCapacity> {
            InputEdge::<ResidualCapacity> {
                source: edge.source,
                target: edge.target,
                data: ResidualCapacity::new(1),
            }
        })
        .collect_vec();
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
    debug!("[{axis}] instantiating min-cut solver, epsilon 0.25");

    // remove eigenloops especially from contracted regions
    let edge_count_before = edges.len();
    edges.retain(|edge| edge.source != edge.target);
    debug!(
        "[{axis}] eigenloop removal - edge count before {edge_count_before}, after {}",
        edges.len()
    );
    edges.shrink_to_fit();

    debug!("[{axis}] instantiating min-cut solver, epsilon {balance_factor}");
    let mut max_flow_solver = Dinic::from_edge_list(edges, 0, 1);
    debug!("[{axis}] instantiated min-cut solver");
    max_flow_solver.run_with_upper_bound(upper_bound);

    let max_flow = max_flow_solver.max_flow();

    if max_flow.is_err() {
        // Error is returned in case the search is aborted early
        return FlowResult {
            flow: i32::MAX,
            balance: 0.,
            assignment: BitVec::new(),
        };
    }
    let flow = max_flow.expect("max flow computation did not run");

    debug!("[{axis}] computed max flow: {flow}");
    let intermediate_assignment = max_flow_solver
        .assignment(0)
        .expect("max flow computation did not run");

    let left_size = intermediate_assignment.iter().filter(|b| !**b).count() + sources.len() - 1;
    let right_size = intermediate_assignment.iter().filter(|b| **b).count() + targets.len() - 1;
    debug!(
        "[{axis}] assignment has {} total entries",
        left_size + right_size
    );
    debug!("[{axis}] assignment has {right_size} 1-entries");
    debug!("[{axis}] assignment has {left_size} 0-entries");

    let balance = std::cmp::min(left_size, right_size) as f64 / (right_size + left_size) as f64;
    debug!("[{axis}] balance: {balance}");

    let mut assignment = BitVec::with_capacity(coordinates.len());
    assignment.resize(coordinates.len(), false);

    // explode intermediate assigment
    for id in &node_id_list {
        let index = renumbering_table[*id];
        if index == usize::MAX {
            // a disconnected node will be assigned to a semi-random partition
            assignment.set(*id, id % 2 == 0);
        } else {
            assignment.set(*id, intermediate_assignment[index]);
        }
    }

    FlowResult {
        flow,
        balance,
        assignment,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{atomic::AtomicI32, Arc};

    use bitvec::bits;
    use bitvec::prelude::*;
    use itertools::Itertools;

    use crate::{
        geometry::primitives::FPCoordinate,
        inertial_flow::{sub_step, TrivialEdge},
    };

    use super::RotatedComparators;

    #[test]
    fn iterate_with_wrap() {
        let comparators = RotatedComparators::new();

        (0..4).zip(4..8).for_each(|indices| {
            assert_eq!(comparators[indices.0], comparators[indices.1]);
        });
    }

    #[test]
    fn inertial_flow() {
        let edges = vec![
            TrivialEdge {
                source: 0,
                target: 1,
            },
            TrivialEdge {
                source: 1,
                target: 0,
            },
            TrivialEdge {
                source: 0,
                target: 2,
            },
            TrivialEdge {
                source: 2,
                target: 0,
            },
            TrivialEdge {
                source: 1,
                target: 2,
            },
            TrivialEdge {
                source: 2,
                target: 1,
            },
            TrivialEdge {
                source: 2,
                target: 4,
            },
            TrivialEdge {
                source: 4,
                target: 2,
            },
            TrivialEdge {
                source: 3,
                target: 5,
            },
            TrivialEdge {
                source: 5,
                target: 3,
            },
            TrivialEdge {
                source: 4,
                target: 3,
            },
            TrivialEdge {
                source: 3,
                target: 4,
            },
            TrivialEdge {
                source: 4,
                target: 5,
            },
            TrivialEdge {
                source: 5,
                target: 4,
            },
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

        let result = sub_step(3, &edges, &coordinates, &node_id_list, 0.25, upper_bound);
        assert_eq!(result.flow, 1);
        assert_eq!(result.balance, 0.5);
        assert_eq!(result.assignment.len(), 6);
        assert_eq!(result.assignment, bits![0, 0, 0, 1, 1, 1]);
    }
}
