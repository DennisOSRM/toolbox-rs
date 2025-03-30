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

impl<T: RTreeElement> Leaf<T> {
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

/// Trait for elements that can be stored in an RTree
pub trait RTreeElement {
    /// Returns the bounding box of this element
    fn bbox(&self) -> BoundingBox;

    /// Returns the distance from this element to the given coordinate
    fn distance_to(&self, coordinate: &FPCoordinate) -> f64;

    /// Returns the center coordinate of this element
    fn center(&self) -> &FPCoordinate;
}

#[derive(Debug)]
pub struct RTree<T: RTreeElement> {
    leaf_nodes: Vec<Leaf<T>>,
    search_nodes: Vec<SearchNode>,
}

impl<T: RTreeElement + std::clone::Clone> RTree<T> {
    /// Creates a new R-tree from an iterator of elements.
    ///
    /// # Arguments
    ///
    /// * `elements` - Iterator of elements to build the tree from
    ///
    /// # Returns
    ///
    /// A new R-tree instance with the provided data organized in a hierarchical structure
    ///
    /// # Implementation Details
    ///
    /// 1. Creates leaf nodes with up to LEAF_PACK_FACTOR elements
    /// 2. Builds interior nodes with up to BRANCHING_FACTOR children
    /// 3. Constructs tree bottom-up level by level
    #[must_use]
    pub fn from_elements<I>(elements: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        info!("Creating RTree from elements");

        info!("sorting by z-order");
        let mut elements: Vec<_> = elements.into_iter().collect();
        elements.sort_by(|a, b| zorder_cmp(a.center(), b.center()));

        // Create leaf nodes
        let leaf_nodes = elements
            .chunks(LEAF_PACK_FACTOR)
            .map(|chunk| {
                let bbox = chunk.iter().fold(BoundingBox::invalid(), |mut acc, elem| {
                    acc.extend_with(&elem.bbox());
                    acc
                });
                Leaf::new(bbox, chunk.to_vec())
            })
            .collect::<Vec<_>>();

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
    /// * The nearest element
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
    pub fn nearest(&self, input_coordinate: &FPCoordinate) -> Result<(T, f64), RTreeError>
    where
        T: Clone,
    {
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
                            let dist = elem.distance_to(input_coordinate);
                            queue.push(QueueElement::new(
                                dist,
                                child_start_index,
                                QueueNodeType::Candidate(elem_idx),
                            ));
                        }
                    }
                }
                QueueNodeType::Candidate(offset) => {
                    let element = self.leaf_nodes[child_start_index].elements()[offset].clone();
                    return Ok((element, distance));
                }
            }
        }
        Err(RTreeError::EmptyTree)
    }
}

// Implement RTreeElement for the original (FPCoordinate, PartitionID) tuple
impl RTreeElement for (FPCoordinate, PartitionID) {
    fn bbox(&self) -> BoundingBox {
        BoundingBox::from_coordinate(&self.0)
    }

    fn distance_to(&self, coordinate: &FPCoordinate) -> f64 {
        self.0.distance_to(coordinate)
    }

    fn center(&self) -> &FPCoordinate {
        &self.0
    }
}
