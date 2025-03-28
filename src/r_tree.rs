use log::{debug, info};
use std::{cmp::Ordering, collections::BinaryHeap};

const BRANCHING_FACTOR: usize = 30;
const LEAF_PACK_FACTOR: usize = 30;

use crate::{
    bounding_box::BoundingBox, geometry::primitives::FPCoordinate, partition::PartitionID,
    space_filling_curve::zorder_cmp,
};

pub struct Leaf<T> {
    bbox: BoundingBox,
    elements: Vec<T>,
}

#[derive(Clone, Copy, Debug)]
pub struct LeafNode {
    bbox: BoundingBox,
    index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct TreeNode {
    bbox: BoundingBox,
    index: usize,
}

#[derive(Clone, Copy, Debug)]
enum SearchNode {
    LeafNode(LeafNode),
    TreeNode(TreeNode),
}

pub struct RTree {
    leaf_nodes: Vec<Leaf<(FPCoordinate, PartitionID)>>,
    search_nodes: Vec<SearchNode>,
}

#[derive(Debug, PartialEq)]
enum QueueNodeType {
    TreeNode,
    LeafNode,
    Candidate(usize),
}

#[derive(Debug)]
struct QueueElement {
    distance: f64,
    child_start_index: usize,
    node_type: QueueNodeType,
}

impl QueueElement {
    /// Creates a new queue element for the R-tree search.
    ///
    /// # Arguments
    ///
    /// * `distance` - Minimum possible distance to search point
    /// * `child_start_index` - Starting index of children in the node array
    /// * `node_type` - Type of the node (Tree, Leaf or Candidate)
    pub fn new(distance: f64, child_start_index: usize, node_type: QueueNodeType) -> Self {
        Self {
            distance,
            child_start_index,
            node_type,
        }
    }
}

impl PartialEq for QueueElement {
    fn eq(&self, other: &Self) -> bool {
        self.distance == other.distance
    }
}

impl PartialOrd for QueueElement {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for QueueElement {}

impl Ord for QueueElement {
    fn cmp(&self, other: &Self) -> Ordering {
        other.distance.partial_cmp(&self.distance).unwrap()
    }
}

impl RTree {
    /// Creates a new R-tree from slices of coordinates and partition IDs.
    ///
    /// # Arguments
    ///
    /// * `coordinates` - Slice of coordinates to build the tree from
    /// * `partition_ids` - Slice of partition IDs corresponding to the coordinates
    ///
    /// # Returns
    ///
    /// A new R-tree instance with the provided data organized in a hierarchical structure
    ///
    /// # Implementation Details
    ///
    /// 1. Sorts elements by z-order curve for spatial locality
    /// 2. Creates leaf nodes with up to LEAF_PACK_FACTOR elements
    /// 3. Builds interior nodes with up to BRANCHING_FACTOR children
    /// 4. Constructs tree bottom-up level by level
    pub fn from_slices(coordinates: &[FPCoordinate], partition_ids: &[PartitionID]) -> Self {
        info!("Creating RTree from slices");
        let mut elements: Vec<(FPCoordinate, PartitionID)> = coordinates
            .iter()
            .zip(partition_ids.iter())
            .map(|(coord, &id)| (*coord, id))
            .collect();
        info!("Sorting elements by z-order");
        elements.sort_by(|a, b| zorder_cmp(&a.0, &b.0));

        info!("Creating leaf nodes");
        let leaf_nodes = elements
            .chunks(LEAF_PACK_FACTOR)
            .map(|chunk| {
                let bbox = BoundingBox::from_coordinates(
                    &chunk.iter().map(|(coord, _)| *coord).collect::<Vec<_>>(),
                );
                Leaf {
                    bbox,
                    elements: chunk.to_vec(),
                }
            })
            .collect::<Vec<_>>();

        debug!("Created {} leaf nodes", leaf_nodes.len());

        let mut search_nodes: Vec<_> = leaf_nodes
            .chunks(BRANCHING_FACTOR)
            .enumerate()
            .map(|(index, chunk)| {
                let bbox = chunk.iter().fold(BoundingBox::invalid(), |acc, leaf| {
                    let mut bbox = acc;
                    bbox.extend_with(&leaf.bbox);
                    bbox
                });
                SearchNode::LeafNode(LeafNode {
                    bbox,
                    index: BRANCHING_FACTOR * index,
                })
            })
            .collect();

        debug!("Created {} search nodes", search_nodes.len());

        let mut start = 0;
        let mut end = search_nodes.len();

        let mut level = 0;
        let mut next = Vec::new();
        debug!("Creating tree nodes, start {start}, end {end}");
        while start < end - 1 {
            debug!(
                "level: {}, packing {} nodes [{}]",
                level,
                search_nodes.len(),
                (end - start)
            );
            level += 1;
            search_nodes[start..end]
                .chunks(BRANCHING_FACTOR)
                .enumerate()
                .for_each(|(index, node)| {
                    let bbox = node.iter().fold(BoundingBox::invalid(), |acc, node| {
                        let mut bbox = acc;
                        match node {
                            SearchNode::LeafNode(leaf) => {
                                bbox.extend_with(&leaf.bbox);
                            }
                            SearchNode::TreeNode(tree) => {
                                bbox.extend_with(&tree.bbox);
                            }
                        }
                        bbox
                    });
                    next.push(SearchNode::TreeNode(TreeNode {
                        bbox,
                        index: start + (BRANCHING_FACTOR * index),
                    }));
                });
            start = end;
            end += next.len();
            search_nodes.append(&mut next);
            next.clear();
        }

        debug!("Created {} search nodes", search_nodes.len());

        RTree {
            leaf_nodes,
            search_nodes,
        }
    }

    /// Finds the nearest neighbor to the given coordinate in the R-tree.
    ///
    /// # Arguments
    ///
    /// * `input_coordinate` - The coordinate to find the nearest neighbor for
    ///
    /// # Returns
    ///
    /// Option containing:
    /// * The nearest coordinate and its partition ID
    /// * The distance to the nearest neighbor
    ///
    /// Returns None if the tree is empty
    ///
    /// # Algorithm
    ///
    /// Uses a priority queue based branch-and-bound search:
    /// 1. Starts from root node
    /// 2. Maintains queue of nodes sorted by minimum possible distance
    /// 3. Explores most promising nodes first
    /// 4. Returns first found point that must be nearest neighbor
    pub fn nearest(
        &self,
        input_coordinate: &FPCoordinate,
    ) -> Option<((FPCoordinate, PartitionID), f64)> {
        debug!("searching for nearest neighbor: {:?}", input_coordinate);
        let root = *self.search_nodes.last().unwrap();

        let (distance, child_start_index) = match root {
            SearchNode::TreeNode(tree) => (tree.bbox.min_distance(input_coordinate), tree.index),
            _ => unreachable!("last entry of search nodes covers the whole tree"),
        };
        let mut nearest = None;
        let mut min_distance = f64::MAX;

        let mut queue = BinaryHeap::<QueueElement>::new();
        queue.push(QueueElement::new(
            distance,
            child_start_index,
            QueueNodeType::TreeNode,
        ));
        struct Stats {
            pushs: usize,
            pops: usize,
        }
        let mut stats = Stats { pushs: 0, pops: 0 };
        stats.pushs += 1;

        while let Some(current_element) = queue.pop() {
            stats.pops += 1;
            debug!(
                "popped node: {:?} with index: {}, start_index: {}, distance {}",
                current_element.node_type,
                current_element.child_start_index,
                current_element.child_start_index,
                current_element.distance
            );

            match current_element.node_type {
                QueueNodeType::TreeNode => {
                    let number_of_children = BRANCHING_FACTOR
                        .min(self.search_nodes.len() - 1 - current_element.child_start_index);
                    (0..number_of_children).for_each(|offset| {
                        let child = &self.search_nodes[current_element.child_start_index + offset];
                        let (node_type, distance, child_start_index) = match child {
                            SearchNode::LeafNode(leaf_node) => (
                                QueueNodeType::LeafNode,
                                leaf_node.bbox.min_distance(input_coordinate),
                                leaf_node.index,
                            ),
                            SearchNode::TreeNode(tree_node) => (
                                QueueNodeType::TreeNode,
                                tree_node.bbox.min_distance(input_coordinate),
                                tree_node.index,
                            ),
                        };
                        queue.push(QueueElement::new(distance, child_start_index, node_type));
                        stats.pushs += 1;
                    });
                }
                QueueNodeType::LeafNode => {
                    for index in 0..LEAF_PACK_FACTOR {
                        self.leaf_nodes[current_element.child_start_index + index]
                            .elements
                            .iter()
                            .enumerate()
                            .for_each(|(offset, candidate)| {
                                let distance = input_coordinate.distance_to(&candidate.0);
                                queue.push(QueueElement::new(
                                    distance,
                                    current_element.child_start_index,
                                    QueueNodeType::Candidate(offset),
                                ));
                                stats.pushs += 1;
                            });
                    }
                }
                QueueNodeType::Candidate(offset) => {
                    let (candidate_coordinate, candidate_id) =
                        self.leaf_nodes[current_element.child_start_index].elements[offset];
                    debug!(
                        " searching candidate: {candidate_coordinate:?} with id {candidate_id:?}"
                    );
                    let distance = input_coordinate.distance_to(&candidate_coordinate);
                    if distance < min_distance {
                        min_distance = distance;
                        nearest = Some(((candidate_coordinate, candidate_id), min_distance));
                        debug!("push: {}, pops: {}", stats.pushs, stats.pops);
                        debug!(
                            "found nearest neighbor: {nearest:?} at distance {min_distance}, queue.len: {}",
                            queue.len()
                        );
                        return nearest;
                    }
                }
            }
        }
        nearest
    }
}
