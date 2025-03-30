use log::debug;
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
    #[error("Invalid coordinate")]
    InvalidCoordinate,
    #[error("Node index out of bounds: {0}")]
    InvalidNodeIndex(usize),
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
        let mut elements: Vec<_> = elements.into_iter().collect();
        debug!("Creating R-tree from {} elements", elements.len());
        debug!("sorting by z-order");
        elements.sort_by(|a, b| zorder_cmp(a.center(), b.center()));

        let estimated_leaf_nodes = (elements.len() + LEAF_PACK_FACTOR - 1) / LEAF_PACK_FACTOR;
        let estimated_search_nodes = estimated_leaf_nodes * 2; // Rough estimate for tree structure

        let mut search_nodes = Vec::with_capacity(estimated_search_nodes);
        let mut next = Vec::with_capacity(estimated_leaf_nodes);

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

        search_nodes.extend(leaf_nodes.chunks(BRANCHING_FACTOR).enumerate().map(
            |(index, chunk)| {
                let bbox = chunk.iter().fold(BoundingBox::invalid(), |acc, leaf| {
                    let mut bbox = acc;
                    bbox.extend_with(leaf.bbox());
                    bbox
                });
                SearchNode::LeafNode(LeafNode::new(bbox, BRANCHING_FACTOR * index))
            },
        ));

        debug!("Created {} search nodes", search_nodes.len());

        let mut start = 0;
        let mut end = search_nodes.len();

        let mut level = 0;
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
        debug!(
            "Created tree with {} levels, {} leaf nodes, {} total nodes",
            level,
            leaf_nodes.len(),
            search_nodes.len()
        );

        RTree {
            leaf_nodes,
            search_nodes,
        }
    }

    /// Returns an iterator over elements in ascending order of distance from the given coordinate
    pub fn nearest_iter<'a>(&'a self, coordinate: &'a FPCoordinate) -> RTreeNearestIterator<'a, T> {
        RTreeNearestIterator::new(self, coordinate)
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

#[derive(Debug)]
pub struct RTreeNearestIterator<'a, T: RTreeElement> {
    tree: &'a RTree<T>,
    input_coordinate: &'a FPCoordinate,
    queue: BinaryHeap<QueueElement>,
}

impl<'a, T: RTreeElement> RTreeNearestIterator<'a, T> {
    fn new(tree: &'a RTree<T>, input_coordinate: &'a FPCoordinate) -> Self {
        let capacity = (tree.leaf_nodes.len() * LEAF_PACK_FACTOR).sqrt();
        let mut queue = BinaryHeap::with_capacity(capacity);

        // Initialize with root node if tree is not empty
        if let Some(SearchNode::TreeNode(root)) = tree.search_nodes.last() {
            queue.push(QueueElement::new(
                root.bbox.min_distance(input_coordinate),
                root.index,
                QueueNodeType::TreeNode,
            ));
        }

        Self {
            tree,
            input_coordinate,
            queue,
        }
    }
}

impl<'a, T: RTreeElement + Clone> Iterator for RTreeNearestIterator<'a, T> {
    /// Returns the next nearest element and its distance from the query point.
    /// Elements are returned in ascending order of distance.
    ///
    /// # Returns
    /// * `Some((element, distance))` - The next nearest element and its distance
    /// * `None` - When all elements have been visited
    type Item = (T, f64);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(QueueElement {
            distance,
            child_start_index,
            node_type,
        }) = self.queue.pop()
        {
            match node_type {
                QueueNodeType::TreeNode => {
                    let children_count =
                        BRANCHING_FACTOR.min(self.tree.search_nodes.len() - 1 - child_start_index);
                    for i in 0..children_count {
                        match &self.tree.search_nodes[child_start_index + i] {
                            SearchNode::LeafNode(node) => self.queue.push(QueueElement::new(
                                node.bbox.min_distance(&self.input_coordinate),
                                node.index,
                                QueueNodeType::LeafNode,
                            )),
                            SearchNode::TreeNode(node) => self.queue.push(QueueElement::new(
                                node.bbox.min_distance(&self.input_coordinate),
                                node.index,
                                QueueNodeType::TreeNode,
                            )),
                        }
                    }
                }
                QueueNodeType::LeafNode => {
                    for leaf_idx in 0..LEAF_PACK_FACTOR {
                        let leaf = &self.tree.leaf_nodes[child_start_index + leaf_idx];
                        for (elem_idx, elem) in leaf.elements().iter().enumerate() {
                            let dist = elem.distance_to(&self.input_coordinate);
                            self.queue.push(QueueElement::new(
                                dist,
                                child_start_index,
                                QueueNodeType::Candidate(elem_idx),
                            ));
                        }
                    }
                }
                QueueNodeType::Candidate(offset) => {
                    let element =
                        self.tree.leaf_nodes[child_start_index].elements()[offset].clone();
                    return Some((element, distance));
                }
            }
        }
        None
    }
}
