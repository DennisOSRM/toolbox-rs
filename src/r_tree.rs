use log::{debug, info};
use num::integer::Roots;
use std::{cmp::Ordering, collections::BinaryHeap};
use thiserror::Error;

const BRANCHING_FACTOR: usize = 30;
const LEAF_PACK_FACTOR: usize = 30;

use crate::{
    bounding_box::BoundingBox, geometry::FPCoordinate, partition::PartitionID,
    space_filling_curve::zorder_cmp,
};

#[derive(Error, Debug)]
pub enum RTreeError {
    #[error("Empty tree")]
    EmptyTree,
}

#[derive(Clone, Debug)]
pub struct Leaf<T> {
    bbox: BoundingBox,
    elements: Vec<T>,
}

impl<T> Leaf<T> {
    pub fn new(bbox: BoundingBox, elements: Vec<T>) -> Self {
        Self { bbox, elements }
    }

    #[must_use]
    pub fn bbox(&self) -> &BoundingBox {
        &self.bbox
    }

    #[must_use]
    pub fn elements(&self) -> &[T] {
        &self.elements
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeafNode {
    bbox: BoundingBox,
    index: usize,
}

impl LeafNode {
    pub fn new(bbox: BoundingBox, index: usize) -> Self {
        Self { bbox, index }
    }
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

#[derive(Debug)]
pub struct RTree {
    leaf_nodes: Vec<Leaf<(FPCoordinate, PartitionID)>>,
    search_nodes: Vec<SearchNode>,
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
    #[must_use]
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
                Leaf::new(bbox, chunk.to_vec())
            })
            .collect::<Vec<_>>();

        debug!("Created {} leaf nodes", leaf_nodes.len());

        let mut search_nodes: Vec<_> = leaf_nodes
            .chunks(BRANCHING_FACTOR)
            .enumerate()
            .map(|(index, chunk)| {
                let bbox = chunk.iter().fold(BoundingBox::invalid(), |acc, leaf| {
                    let mut bbox = acc;
                    bbox.extend_with(leaf.bbox());
                    bbox
                });
                SearchNode::LeafNode(LeafNode::new(bbox, BRANCHING_FACTOR * index))
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
    #[must_use]
    pub fn nearest(
        &self,
        input_coordinate: &FPCoordinate,
    ) -> Result<((FPCoordinate, PartitionID), f64), RTreeError> {
        let root = self.search_nodes.last().ok_or(RTreeError::EmptyTree)?;

        let (root_distance, root_index) = match root {
            SearchNode::TreeNode(tree) => (tree.bbox.min_distance(input_coordinate), tree.index),
            _ => unreachable!("last entry of search nodes covers the whole tree"),
        };

        let mut queue =
            BinaryHeap::with_capacity((self.leaf_nodes.len() * LEAF_PACK_FACTOR).sqrt());
        queue.push(QueueElement::new(
            root_distance,
            root_index,
            QueueNodeType::TreeNode,
        ));

        while let Some(QueueElement {
            distance,
            child_start_index,
            node_type,
        }) = queue.pop()
        {
            match node_type {
                QueueNodeType::TreeNode => {
                    let children_count =
                        BRANCHING_FACTOR.min(self.search_nodes.len() - 1 - child_start_index);
                    for i in 0..children_count {
                        match &self.search_nodes[child_start_index + i] {
                            SearchNode::LeafNode(node) => queue.push(QueueElement::new(
                                node.bbox.min_distance(input_coordinate),
                                node.index,
                                QueueNodeType::LeafNode,
                            )),
                            SearchNode::TreeNode(node) => queue.push(QueueElement::new(
                                node.bbox.min_distance(input_coordinate),
                                node.index,
                                QueueNodeType::TreeNode,
                            )),
                        }
                    }
                }
                QueueNodeType::LeafNode => {
                    for leaf_idx in 0..LEAF_PACK_FACTOR {
                        let leaf = &self.leaf_nodes[child_start_index + leaf_idx];
                        for (elem_idx, elem) in leaf.elements().iter().enumerate() {
                            let dist = input_coordinate.distance_to(&elem.0);
                            queue.push(QueueElement::new(
                                dist,
                                child_start_index,
                                QueueNodeType::Candidate(elem_idx),
                            ));
                        }
                    }
                }
                QueueNodeType::Candidate(offset) => {
                    let (coord, id) = self.leaf_nodes[child_start_index].elements()[offset];
                    // Since queue is ordered by distance, first candidate is guaranteed to be nearest
                    return Ok(((coord, id), distance));
                }
            }
        }
        Err(RTreeError::EmptyTree)
    }
}
