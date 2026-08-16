use log::debug;
use num::integer::Roots;
use std::{cmp::Ordering, collections::BinaryHeap};
use thiserror::Error;

const BRANCHING_FACTOR: usize = 30;
const LEAF_PACK_FACTOR: usize = 30;

use crate::{
    bounding_box::BoundingBox, geometry::FPCoordinate, partition_id::PartitionID,
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
    /// How many children sit at `index`, which is not always the branching
    /// factor: the last node of a level takes what is left of it. Reading a
    /// fixed number instead runs off the end of the level being packed and
    /// into the one being built on top of it, and a node then has its own
    /// parent among its children, which is a walk that never ends.
    children: usize,
}

#[derive(Clone, Copy, Debug)]
enum SearchNode {
    LeafNode(LeafNode),
    TreeNode(TreeNode),
}

#[derive(Debug, PartialEq)]
enum QueueNodeType {
    /// a node, holding how many children sit at its start index
    TreeNode(usize),
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
        if elements.is_empty() {
            // there is no root to pack, and the packing counts down from the
            // number of nodes, which is where an empty tree ran off the bottom
            return RTree {
                leaf_nodes: Vec::new(),
                search_nodes: Vec::new(),
            };
        }
        debug!("sorting by z-order");
        elements.sort_by(|a, b| zorder_cmp(a.center(), b.center()));

        let estimated_leaf_nodes = elements.len().div_ceil(LEAF_PACK_FACTOR);
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
                        children: node.len(),
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

    /// Returns the elements whose box shares ground with the given one, in no
    /// particular order.
    ///
    /// This walks down from the root and passes over any subtree whose box
    /// misses the one asked about, so the work is the answer plus the boxes
    /// along the way rather than everything in the tree.
    ///
    /// The nearest iterator can be made to answer the same question, by taking
    /// elements until they are further away than the far corner of the box, but
    /// it is the wrong shape for it: it pays a heap to put in an order that is
    /// then thrown away, it reaches over a disc where a box was asked for, and
    /// the distance it orders by is to the middle of an element rather than to
    /// the nearest part of it, so a large element whose middle lies far off
    /// comes late or not at all. A cell of a partition that covers a country is
    /// exactly that element.
    pub fn intersecting<'a>(&'a self, bbox: &'a BoundingBox) -> RTreeIntersectionIterator<'a, T> {
        RTreeIntersectionIterator::new(self, bbox)
    }
}

/// Walks the elements whose box shares ground with the one asked about.
#[derive(Debug)]
pub struct RTreeIntersectionIterator<'a, T: RTreeElement> {
    tree: &'a RTree<T>,
    bbox: &'a BoundingBox,
    /// the subtrees still to look at
    stack: Vec<SearchNode>,
    /// the leaves whose box shares ground and whose elements are still to come
    leaves: Vec<usize>,
    /// the leaf being handed out, and where in it we are
    leaf: Option<(usize, usize)>,
}

impl<'a, T: RTreeElement> RTreeIntersectionIterator<'a, T> {
    fn new(tree: &'a RTree<T>, bbox: &'a BoundingBox) -> Self {
        let mut stack = Vec::new();
        if let Some(&root) = tree.search_nodes.last() {
            stack.push(root);
        }
        Self {
            tree,
            bbox,
            stack,
            leaves: Vec::new(),
            leaf: None,
        }
    }
}

impl<'a, T: RTreeElement> Iterator for RTreeIntersectionIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // hand out what is left of the leaf that was opened last
            if let Some((at, offset)) = self.leaf {
                let elements = self.tree.leaf_nodes[at].elements();
                if offset < elements.len() {
                    self.leaf = Some((at, offset + 1));
                    let element = &elements[offset];
                    if element.bbox().intersects(self.bbox) {
                        return Some(element);
                    }
                    continue;
                }
                self.leaf = None;
            }
            if let Some(at) = self.leaves.pop() {
                self.leaf = Some((at, 0));
                continue;
            }

            match self.stack.pop()? {
                SearchNode::TreeNode(node) => {
                    if !node.bbox.intersects(self.bbox) {
                        continue;
                    }
                    for child in 0..node.children {
                        self.stack.push(self.tree.search_nodes[node.index + child]);
                    }
                }
                SearchNode::LeafNode(node) => {
                    if !node.bbox.intersects(self.bbox) {
                        continue;
                    }
                    // the leaves of this chunk whose box shares ground, kept
                    // apart from the subtrees as they are read one element at a
                    // time rather than opened up
                    for offset in 0..BRANCHING_FACTOR {
                        let at = node.index + offset;
                        if at >= self.tree.leaf_nodes.len() {
                            break;
                        }
                        if self.tree.leaf_nodes[at].bbox().intersects(self.bbox) {
                            self.leaves.push(at);
                        }
                    }
                }
            }
        }
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
        if let Some(last_node) = tree.search_nodes.last() {
            match last_node {
                SearchNode::TreeNode(root) => {
                    queue.push(QueueElement::new(
                        root.bbox.min_distance(input_coordinate),
                        root.index,
                        QueueNodeType::TreeNode(root.children),
                    ));
                }
                SearchNode::LeafNode(leaf) => {
                    queue.push(QueueElement::new(
                        leaf.bbox.min_distance(input_coordinate),
                        leaf.index,
                        QueueNodeType::LeafNode,
                    ));
                }
            }
        }

        Self {
            tree,
            input_coordinate,
            queue,
        }
    }
}

impl<T: RTreeElement + Clone> Iterator for RTreeNearestIterator<'_, T> {
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
                QueueNodeType::TreeNode(children) => {
                    for i in 0..children {
                        match &self.tree.search_nodes[child_start_index + i] {
                            SearchNode::LeafNode(node) => self.queue.push(QueueElement::new(
                                node.bbox.min_distance(self.input_coordinate),
                                node.index,
                                QueueNodeType::LeafNode,
                            )),
                            SearchNode::TreeNode(node) => self.queue.push(QueueElement::new(
                                node.bbox.min_distance(self.input_coordinate),
                                node.index,
                                QueueNodeType::TreeNode(node.children),
                            )),
                        }
                    }
                }
                QueueNodeType::LeafNode => {
                    // Only iterate over valid leaf nodes in this chunk
                    let max_leaf = self.tree.leaf_nodes.len();
                    for leaf_idx in 0..LEAF_PACK_FACTOR {
                        let idx = child_start_index + leaf_idx;
                        if idx >= max_leaf {
                            break;
                        }
                        let leaf = &self.tree.leaf_nodes[idx];
                        for (elem_idx, elem) in leaf.elements().iter().enumerate() {
                            let dist = elem.distance_to(self.input_coordinate);
                            self.queue.push(QueueElement::new(
                                dist,
                                idx,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounding_box::BoundingBox;
    use crate::geometry::FPCoordinate;
    use crate::partition_id::PartitionID;

    #[derive(Clone, Debug, PartialEq)]
    struct DummyElem(FPCoordinate);
    impl RTreeElement for DummyElem {
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

    #[test]
    fn test_leaf_new_and_accessors() {
        let coord = FPCoordinate::new_from_lat_lon(1.0, 2.0);
        let bbox = BoundingBox::from_coordinate(&coord);
        let elem = DummyElem(coord);
        let leaf = Leaf::new(bbox, vec![elem.clone()]);
        assert_eq!(leaf.bbox(), &bbox);
        assert_eq!(leaf.elements(), &[elem]);
    }

    #[test]
    fn test_leafnode_new() {
        let bbox = BoundingBox::from_coordinate(&FPCoordinate::new_from_lat_lon(0.0, 0.0));
        let node = LeafNode::new(bbox, 42);
        assert_eq!(node.bbox, bbox);
        assert_eq!(node.index, 42);
    }

    #[test]
    fn test_queuenode_ordering() {
        let a = QueueElement::new(1.0, 0, QueueNodeType::TreeNode(0));
        let b = QueueElement::new(2.0, 1, QueueNodeType::TreeNode(0));
        assert!(a > b); // Because BinaryHeap is max-heap, but we want min-heap
    }

    #[test]
    fn test_rtree_from_elements_and_nearest_iter() {
        // Insert at least LEAF_PACK_FACTOR * 2 + 1 elements to ensure multiple leaf nodes and a tree node
        let mut coords = Vec::new();
        for i in 0..(LEAF_PACK_FACTOR * 2 + 1) {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords.clone());
        let query = FPCoordinate::new_from_lat_lon(5.1, 5.1);
        let mut iter = tree.nearest_iter(&query);

        // Find the true nearest element by brute force
        let (true_nearest, true_dist) = coords
            .iter()
            .map(|e| (e.clone(), e.distance_to(&query)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();

        // Collect all elements from the iterator and their distances
        let iter_results: Vec<_> = iter.by_ref().collect();
        assert_eq!(iter_results.len(), coords.len());

        // The first element should be the true nearest
        assert_eq!(iter_results[0].0, true_nearest);
        assert!((iter_results[0].1 - true_dist).abs() < 1e-6);

        // All elements should be sorted by distance
        for i in 1..iter_results.len() {
            assert!(iter_results[i - 1].1 <= iter_results[i].1);
        }
    }

    #[test]
    fn test_rtreeelement_for_tuple() {
        let coord = FPCoordinate::new_from_lat_lon(3.0, 4.0);
        let pid = PartitionID(7);
        let tuple = (coord, pid);
        assert_eq!(tuple.bbox(), BoundingBox::from_coordinate(&coord));
        assert_eq!(tuple.center(), &coord);
        let origin = FPCoordinate::new_from_lat_lon(0.0, 0.0);
        assert!(tuple.distance_to(&origin) > 0.0);
    }

    #[test]
    fn test_leaf_fills_to_capacity() {
        // Fill exactly one leaf node
        let mut coords = Vec::new();
        for i in 0..LEAF_PACK_FACTOR {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords.clone());
        // There should be exactly one leaf node
        assert_eq!(tree.leaf_nodes.len(), 1);
        // The leaf node should contain all elements
        let leaf = &tree.leaf_nodes[0];
        assert_eq!(leaf.elements().len(), LEAF_PACK_FACTOR);
        // All inserted elements should be present
        for elem in &coords {
            assert!(leaf.elements().iter().any(|e| e.0 == elem.0));
        }
    }

    #[test]
    fn test_multiple_leafs_filled_to_capacity() {
        // Fill several leaf nodes
        let num_leaves = 4;
        let num_elements = LEAF_PACK_FACTOR * num_leaves;
        let mut coords = Vec::new();
        for i in 0..num_elements {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords.clone());
        // There should be exactly num_leaves leaf nodes
        assert_eq!(tree.leaf_nodes.len(), num_leaves);
        // Each leaf node should contain exactly LEAF_PACK_FACTOR elements
        for leaf in &tree.leaf_nodes {
            assert_eq!(leaf.elements().len(), LEAF_PACK_FACTOR);
        }
        // All inserted elements should be present in some leaf
        let all_leaf_elements = tree
            .leaf_nodes
            .iter()
            .flat_map(|leaf| leaf.elements())
            .collect::<Vec<_>>();
        for elem in &coords {
            assert!(all_leaf_elements.iter().any(|e| e.0 == elem.0));
        }
    }

    #[test]
    fn test_rtree_from_elements_leaf_and_tree_structure() {
        // Insert enough elements to create multiple leaves and at least one tree node
        let num_elements = LEAF_PACK_FACTOR * BRANCHING_FACTOR + 1;
        let mut coords = Vec::new();
        for i in 0..num_elements {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords.clone());
        // Check that the number of leaf nodes is as expected
        let expected_leaves = num_elements.div_ceil(LEAF_PACK_FACTOR);
        assert_eq!(tree.leaf_nodes.len(), expected_leaves);
        // Check that each leaf node has at most LEAF_PACK_FACTOR elements
        for (i, leaf) in tree.leaf_nodes.iter().enumerate() {
            if i < expected_leaves - 1 {
                assert_eq!(leaf.elements().len(), LEAF_PACK_FACTOR);
            } else {
                // The last leaf may have fewer elements
                assert!(leaf.elements().len() <= LEAF_PACK_FACTOR);
            }
        }
        // Check that the search_nodes vector contains both LeafNode and TreeNode variants
        let mut has_leaf = false;
        let mut has_tree = false;
        for node in &tree.search_nodes {
            match node {
                SearchNode::LeafNode(leaf) => {
                    has_leaf = true;
                    // Check that the index and bbox are valid for the leaf node
                    assert!(leaf.index < tree.leaf_nodes.len() * BRANCHING_FACTOR);
                    let _ = &leaf.bbox; // Access bbox to cover line 221
                }
                SearchNode::TreeNode(tree_node) => {
                    has_tree = true;
                    // Check that the index and bbox are valid for the tree node
                    assert!(tree_node.index < tree.search_nodes.len());
                    let _ = &tree_node.bbox; // Access bbox to cover line 222
                }
            }
        }
        assert!(has_leaf);
        assert!(has_tree);
    }

    #[test]
    fn test_rtree_nearest_iter_init_with_leafnode() {
        // Create a tree with only enough elements for a single leaf node
        let mut coords = Vec::new();
        for i in 0..LEAF_PACK_FACTOR {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords.clone());
        // Force the root to be a LeafNode (happens when only one leaf node exists)
        let query = FPCoordinate::new_from_lat_lon(0.0, 0.0);
        let iter = tree.nearest_iter(&query);
        // The queue should have been initialized with a LeafNode branch (line 286)
        // We check that the iterator returns all elements in the tree
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), coords.len());
        for (elem, dist) in results {
            assert!(coords.iter().any(|c| c.0 == elem.0));
            assert!(dist >= 0.0);
        }
    }

    #[test]
    fn test_queueelement_ordering_and_equality() {
        let a = QueueElement::new(1.0, 0, QueueNodeType::TreeNode(0));
        let b = QueueElement::new(2.0, 1, QueueNodeType::LeafNode);
        let c = QueueElement::new(1.0, 2, QueueNodeType::Candidate(0));

        // Test PartialEq and Eq
        assert_eq!(a, c);
        assert_ne!(a, b);

        // Test PartialOrd and Ord (min-heap behavior)
        assert!(a > b); // Because BinaryHeap is max-heap, but we want min-heap
        assert!(b < a);

        // Test ordering in a BinaryHeap
        use std::collections::BinaryHeap;
        let mut heap = BinaryHeap::new();
        heap.push(a);
        heap.push(b);
        heap.push(c);

        // The element with the smallest distance should be popped last
        let first = heap.pop().unwrap();
        let second = heap.pop().unwrap();
        let third = heap.pop().unwrap();
        assert_eq!(first.distance, 1.0);
        assert_eq!(second.distance, 1.0);
        assert_eq!(third.distance, 2.0);
    }

    #[test]
    fn test_leafnode_index_bounds() {
        // Create enough elements to ensure multiple leaf nodes
        let num_elements = LEAF_PACK_FACTOR * 3;
        let mut coords = Vec::new();
        for i in 0..num_elements {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        let tree = RTree::from_elements(coords);
        // Check that all LeafNode indices are within bounds
        for node in &tree.search_nodes {
            if let SearchNode::LeafNode(leaf) = node {
                assert!(leaf.index < tree.leaf_nodes.len() * BRANCHING_FACTOR);
            }
        }
    }

    #[test]
    fn test_rtree_from_elements_triggers_leafnode_bbox_extend_with() {
        // To guarantee line 220 is hit, we need more than BRANCHING_FACTOR leaf nodes.
        // That means more than LEAF_PACK_FACTOR * BRANCHING_FACTOR elements.
        let num_elements = LEAF_PACK_FACTOR * (BRANCHING_FACTOR + 1);
        let mut coords = Vec::new();
        for i in 0..num_elements {
            coords.push(DummyElem(FPCoordinate::new_from_lat_lon(
                i as f64, i as f64,
            )));
        }
        // This will create more than BRANCHING_FACTOR leaves, so the fold closure will be called for multiple leaves in a chunk.
        let tree = RTree::from_elements(coords);
        // We just assert the number of leaf nodes to ensure the code path is hit.
        assert!(tree.leaf_nodes.len() > BRANCHING_FACTOR);
    }

    /// A box of its own, so that the tree can be asked about elements that
    /// cover ground rather than only about points.
    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Patch {
        low: FPCoordinate,
        high: FPCoordinate,
        /// worked out once, as the trait hands back a reference and there is
        /// nowhere to keep one that is made up on the spot
        center: FPCoordinate,
        name: usize,
    }

    impl RTreeElement for Patch {
        fn bbox(&self) -> BoundingBox {
            BoundingBox::from_coordinates(&[self.low, self.high])
        }
        fn distance_to(&self, coordinate: &FPCoordinate) -> f64 {
            self.bbox().min_distance(coordinate)
        }
        fn center(&self) -> &FPCoordinate {
            &self.center
        }
    }

    fn patch(name: usize, low_lat: i32, low_lon: i32, high_lat: i32, high_lon: i32) -> Patch {
        Patch {
            low: FPCoordinate::new(low_lat, low_lon),
            high: FPCoordinate::new(high_lat, high_lon),
            center: FPCoordinate::new((low_lat + high_lat) / 2, (low_lon + high_lon) / 2),
            name,
        }
    }

    fn window(low_lat: i32, low_lon: i32, high_lat: i32, high_lon: i32) -> BoundingBox {
        BoundingBox::from_coordinates(&[
            FPCoordinate::new(low_lat, low_lon),
            FPCoordinate::new(high_lat, high_lon),
        ])
    }

    /// What the query has to answer, worked out by asking every element in
    /// turn. Anything the tree hands back has to match this exactly: the tree
    /// is only allowed to be quicker, not to differ.
    fn found_by_hand(patches: &[Patch], bbox: &BoundingBox) -> Vec<usize> {
        let mut names = patches
            .iter()
            .filter(|patch| patch.bbox().intersects(bbox))
            .map(|patch| patch.name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    fn found_by_tree(tree: &RTree<Patch>, bbox: &BoundingBox) -> Vec<usize> {
        let mut names = tree
            .intersecting(bbox)
            .map(|patch| patch.name)
            .collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    #[test]
    fn an_empty_tree_hands_back_nothing() {
        let tree = RTree::<Patch>::from_elements(Vec::new());
        assert!(tree.intersecting(&window(0, 0, 10, 10)).next().is_none());
    }

    #[test]
    fn a_window_finds_what_lies_under_it() {
        let patches = vec![
            patch(0, 0, 0, 10, 10),
            patch(1, 100, 100, 110, 110),
            patch(2, 5, 5, 200, 200),
        ];
        let tree = RTree::from_elements(patches.clone());

        // over the first patch only, though the third reaches across it
        assert_eq!(found_by_tree(&tree, &window(0, 0, 1, 1)), vec![0]);
        // over the second, and the third which covers everything
        assert_eq!(
            found_by_tree(&tree, &window(100, 100, 101, 101)),
            vec![1, 2]
        );
        // and nowhere near any of them
        assert!(found_by_tree(&tree, &window(-100, -100, -90, -90)).is_empty());
    }

    /// The case that a nearest first walk is bad at: an element that covers a
    /// great deal of ground, whose middle is far from the window it covers.
    #[test]
    fn a_large_element_is_found_under_a_window_far_from_its_middle() {
        let patches = vec![
            patch(0, -1000, -1000, 1000, 1000),
            patch(1, 900, 900, 901, 901),
        ];
        let tree = RTree::from_elements(patches);

        let corner = window(995, 995, 999, 999);
        assert_eq!(found_by_tree(&tree, &corner), vec![0]);
    }

    #[test]
    fn the_tree_finds_what_a_walk_of_every_element_finds() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_B0B0);
        for round in 0..20 {
            // patches of every size, as a tree that only holds small ones does
            // not have to prune anything worth pruning
            let patches = (0..(40 + round * 10))
                .map(|name| {
                    let lat = rng.random_range(-1000..1000);
                    let lon = rng.random_range(-1000..1000);
                    let height = rng.random_range(1..500);
                    let width = rng.random_range(1..500);
                    patch(name, lat, lon, lat + height, lon + width)
                })
                .collect::<Vec<_>>();
            let tree = RTree::from_elements(patches.clone());

            for _ in 0..20 {
                let lat = rng.random_range(-1200..1200);
                let lon = rng.random_range(-1200..1200);
                let side = rng.random_range(1..300);
                let bbox = window(lat, lon, lat + side, lon + side);
                assert_eq!(
                    found_by_tree(&tree, &bbox),
                    found_by_hand(&patches, &bbox),
                    "round {round}, window {lat},{lon} +{side}"
                );
            }
        }
    }

    /// Nothing is handed out twice, however the leaves are packed.
    #[test]
    fn every_element_comes_back_once() {
        let patches = (0..200)
            .map(|name| {
                let at = name as i32 * 7;
                patch(name, at, at, at + 3, at + 3)
            })
            .collect::<Vec<_>>();
        let tree = RTree::from_elements(patches.clone());

        let everywhere = window(-10_000, -10_000, 10_000, 10_000);
        let found = found_by_tree(&tree, &everywhere);
        assert_eq!(found.len(), patches.len());
        assert_eq!(found, (0..200).collect::<Vec<_>>());
    }

    #[test]
    fn an_empty_tree_has_nothing_to_walk() {
        let tree = RTree::from_elements(Vec::<Patch>::new());
        assert!(
            tree.nearest_iter(&FPCoordinate::new(0, 0)).next().is_none(),
            "a tree of nothing hands back nothing"
        );
    }

    /// A tree of fewer than nine hundred elements is one level deep: thirty to
    /// a leaf and thirty leaves under the one node above them. Everything below
    /// this line is about what happens when there is more than that, which is
    /// the descent itself, and it went untested until this was written.
    fn deep_tree() -> (Vec<Patch>, RTree<Patch>) {
        let patches = (0..5000)
            .map(|name| {
                let lat = (name as i32 % 100) * 20;
                let lon = (name as i32 / 100) * 20;
                // sizes that vary, so that the boxes of the nodes above overlap
                // and the descent has to look at more than one of them
                let span = 5 + (name as i32 % 7) * 10;
                patch(name, lat, lon, lat + span, lon + span)
            })
            .collect::<Vec<_>>();
        let tree = RTree::from_elements(patches.clone());
        (patches, tree)
    }

    #[test]
    fn a_tree_of_more_than_one_level_has_nodes_above_its_leaves() {
        let (patches, tree) = deep_tree();
        assert!(
            patches.len() > LEAF_PACK_FACTOR * BRANCHING_FACTOR,
            "too few to nest"
        );
        assert!(
            matches!(tree.search_nodes.last(), Some(SearchNode::TreeNode(_))),
            "the root of a tree this size is a node rather than a leaf"
        );
        assert!(
            tree.search_nodes
                .iter()
                .any(|node| matches!(node, SearchNode::TreeNode(_))),
            "no interior node was built"
        );
    }

    #[test]
    fn the_descent_finds_what_a_walk_of_every_element_finds() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let (patches, tree) = deep_tree();
        let mut rng = StdRng::seed_from_u64(0x_DEE9);
        for round in 0..40 {
            let lat = rng.random_range(-200..2200);
            let lon = rng.random_range(-200..1200);
            let side = rng.random_range(1..400);
            let bbox = window(lat, lon, lat + side, lon + side);
            assert_eq!(
                found_by_tree(&tree, &bbox),
                found_by_hand(&patches, &bbox),
                "round {round}, window {lat},{lon} +{side}"
            );
        }
    }

    #[test]
    fn a_window_over_the_whole_of_a_deep_tree_finds_all_of_it() {
        let (patches, tree) = deep_tree();
        let everywhere = window(-10_000, -10_000, 10_000, 10_000);
        assert_eq!(found_by_tree(&tree, &everywhere).len(), patches.len());
    }

    #[test]
    fn a_window_beside_a_deep_tree_finds_none_of_it() {
        let (_, tree) = deep_tree();
        let elsewhere = window(-9_000, -9_000, -8_000, -8_000);
        assert!(tree.intersecting(&elsewhere).next().is_none());
    }

    /// The nearest first walk goes down the same nodes, and had no test that
    /// reached them either.
    #[test]
    fn the_nearest_walk_of_a_deep_tree_comes_out_in_order() {
        let (patches, tree) = deep_tree();
        let from = FPCoordinate::new(1000, 500);

        let mut last = 0.0_f64;
        let mut seen = 0;
        for (patch, distance) in tree.nearest_iter(&from).take(200) {
            assert!(distance >= last, "{distance} came after {last}");
            assert!(patch.distance_to(&from) >= 0.0);
            last = distance;
            seen += 1;
        }
        assert_eq!(
            seen,
            200,
            "the walk stopped early over {} elements",
            patches.len()
        );
    }

    /// Three levels rather than two: thirty to a leaf, thirty leaves under a
    /// node, and thirty of those nodes under the root. Only at this size does
    /// the packing put nodes under nodes, and only then does the descent step
    /// through a node whose children are nodes as well.
    fn deeper_tree() -> (Vec<Patch>, RTree<Patch>) {
        let side = LEAF_PACK_FACTOR * BRANCHING_FACTOR * BRANCHING_FACTOR;
        let patches = (0..side + 100)
            .map(|name| {
                let lat = (name as i32 % 200) * 15;
                let lon = (name as i32 / 200) * 15;
                patch(name, lat, lon, lat + 10, lon + 10)
            })
            .collect::<Vec<_>>();
        let tree = RTree::from_elements(patches.clone());
        (patches, tree)
    }

    #[test]
    fn a_tree_of_three_levels_puts_nodes_under_nodes() {
        let (_, tree) = deeper_tree();
        let Some(SearchNode::TreeNode(root)) = tree.search_nodes.last() else {
            panic!("the root of a tree this size is a node");
        };
        assert!(
            matches!(tree.search_nodes[root.index], SearchNode::TreeNode(_)),
            "the children of the root are nodes in their own right"
        );
    }

    #[test]
    fn the_descent_of_three_levels_finds_what_a_walk_finds() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let (patches, tree) = deeper_tree();
        let mut rng = StdRng::seed_from_u64(0x_D33D);
        for round in 0..20 {
            let lat = rng.random_range(-100..3100);
            let lon = rng.random_range(-100..2100);
            let side = rng.random_range(1..500);
            let bbox = window(lat, lon, lat + side, lon + side);
            assert_eq!(
                found_by_tree(&tree, &bbox),
                found_by_hand(&patches, &bbox),
                "round {round}, window {lat},{lon} +{side}"
            );
        }
        // and the whole of it comes back once
        let everywhere = window(-100_000, -100_000, 100_000, 100_000);
        assert_eq!(found_by_tree(&tree, &everywhere).len(), patches.len());
    }

    /// The walk hands elements out nearest first, which holds only if the
    /// distance kept for a node is no further off than anything under it. It
    /// is the deep tree that shows this up: a node of three levels covers
    /// ground wide enough that its corners lie well past its edges.
    #[test]
    fn the_nearest_walk_of_three_levels_comes_out_in_order() {
        let (_, tree) = deeper_tree();
        let from = FPCoordinate::new(1_500, 1_000);
        let mut last = 0.0_f64;
        for (_, distance) in tree.nearest_iter(&from).take(500) {
            assert!(distance >= last, "{distance} came after {last}");
            last = distance;
        }
    }

    #[test]
    fn the_nearest_walk_of_three_levels_reaches_every_element() {
        let (patches, tree) = deeper_tree();
        let from = FPCoordinate::new(1500, 1000);
        let mut seen = tree
            .nearest_iter(&from)
            .map(|(patch, _)| patch.name)
            .collect::<Vec<_>>();
        assert_eq!(seen.len(), patches.len(), "the walk hands out each once");
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), patches.len(), "and none of them twice");
    }
}
