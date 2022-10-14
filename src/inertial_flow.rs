use std::{
    cmp::max,
    ops::Index,
    sync::{atomic::AtomicI32, Arc},
};

use itertools::Itertools;
use log::debug;

use crate::{
    dinic::Dinic,
    edge::{InputEdge, TrivialEdge},
    geometry::primitives::FPCoordinate,
    max_flow::{MaxFlow, ResidualCapacity},
    renumbering_table::RenumberingTable,
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
    pub left_ids: Vec<usize>,
    pub right_ids: Vec<usize>,
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
    input_edges: &[TrivialEdge],
    node_id_list: &[usize],
    coordinates: &[FPCoordinate],
    axis: usize,
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
    let sources = &node_id_list[0..size_of_contraction];
    let targets = &node_id_list[node_id_list.len() - size_of_contraction..];

    debug_assert!(!sources.is_empty());
    debug_assert!(!targets.is_empty());

    debug!("[{axis}] renumbering of inertial flow graph");
    // let mut renumbering_table = vec![usize::MAX; coordinates.len()];
    let mut renumbering_table =
        RenumberingTable::new_with_size_hint(coordinates.len(), node_id_list.len());
    // nodes in the in the graph have to be numbered consecutively.
    // the mapping is input id -> dinic id

    for s in sources {
        renumbering_table.set(*s, 0);
    }
    for t in targets {
        renumbering_table.set(*t, 1);
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
        if !renumbering_table.contains_key(e.source) {
            renumbering_table.set(e.source, current_id);
            current_id += 1;
        }
        if !renumbering_table.contains_key(e.target) {
            renumbering_table.set(e.target, current_id);
            current_id += 1;
        }
        e.source = renumbering_table.get(e.source);
        e.target = renumbering_table.get(e.target);
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
            left_ids: Vec::new(),
            right_ids: Vec::new(),
        };
    }
    let flow = max_flow.expect("max flow computation did not run");

    debug!("[{axis}] computed max flow: {flow}");
    let intermediate_assignment = max_flow_solver
        .assignment(0)
        .expect("max flow computation did not run");

    // TODO: don't copy, but partition in place
    let (left_ids, right_ids): (Vec<_>, Vec<_>) = node_id_list
        .into_iter()
        .filter(|id| renumbering_table.contains_key(*id))
        .partition(|id| intermediate_assignment[renumbering_table.get(*id)]);

    debug_assert!(!left_ids.is_empty());
    debug_assert!(!right_ids.is_empty());

    let balance = std::cmp::min(left_ids.len(), right_ids.len()) as f64
        / (left_ids.len() + right_ids.len()) as f64;
    debug!("[{axis}] balance: {balance}");

    FlowResult {
        flow,
        balance,
        left_ids,
        right_ids,
    }
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use std::sync::{atomic::AtomicI32, Arc};

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

        let result = sub_step(&edges, &node_id_list, &coordinates, 3, 0.25, upper_bound);
        assert_eq!(result.flow, 1);
        assert_eq!(result.balance, 0.5);
        assert_eq!(result.left_ids.len(), 3);
        assert_eq!(result.left_ids, vec![4, 5, 3]);
        assert_eq!(result.right_ids.len(), 3);
        assert_eq!(result.right_ids, vec![2, 0, 1]);
    }
}
