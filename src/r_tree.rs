use core::panic;
use std::{cmp::Ordering, collections::BinaryHeap};

use log::info;

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
    Candidate((FPCoordinate, PartitionID)),
}

pub struct RTree {
    leaf_nodes: Vec<Leaf<(FPCoordinate, PartitionID)>>,
    search_nodes: Vec<SearchNode>,
}

#[derive(Debug)]
struct QueueElement {
    node: SearchNode,
    distance: f64,
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
    pub fn from_slices(coordinates: &[FPCoordinate], partition_ids: &[PartitionID]) -> Self {
        info!("Creating RTree from slices");
        let mut elements: Vec<(FPCoordinate, PartitionID)> = coordinates
            .iter()
            .zip(partition_ids.iter())
            .map(|(coord, &id)| (coord.clone(), id.into()))
            .collect();
        info!("Sorting elements by z-order");
        elements.sort_by(|a, b| zorder_cmp(&a.0, &b.0));

        info!("Creating leaf nodes");
        // a hundred coordinates go into a leaf node (for now)
        let leaf_nodes = elements
            .chunks(100)
            .map(|chunk| {
                let bbox = BoundingBox::from_coordinates(
                    &chunk
                        .iter()
                        .map(|(coord, _)| coord.clone())
                        .collect::<Vec<_>>(),
                );
                Leaf {
                    bbox,
                    elements: chunk.to_vec(),
                }
            })
            .collect::<Vec<_>>();

        println!("Created {} leaf nodes", leaf_nodes.len());
        println!(
            "leaf bbox[0]: {:?}",
            geojson::Bbox::from(&leaf_nodes[0].bbox)
        );
        println!(
            "leaf bbox[1]: {:?}",
            geojson::Bbox::from(&leaf_nodes[1].bbox)
        );

        let mut search_nodes: Vec<_> = leaf_nodes
            .chunks(50)
            .enumerate()
            .map(|(index, chunk)| {
                let bbox = chunk.iter().fold(BoundingBox::invalid(), |acc, leaf| {
                    let mut bbox = acc;
                    bbox.extend_with(&leaf.bbox);
                    bbox
                });
                SearchNode::LeafNode(LeafNode {
                    bbox,
                    index: 50 * index,
                })
            })
            .collect();

        println!("Created {} search nodes", search_nodes.len());
        match &search_nodes[0] {
            SearchNode::LeafNode(leaf) => {
                println!(
                    "search bbox: {:?}, index: {}",
                    geojson::Bbox::from(&leaf.bbox),
                    leaf.index
                );
            }
            _ => {
                unreachable!("first layer of search nodes should be leaf nodes")
            }
        }

        let mut start = 0;
        let mut end = search_nodes.len();

        let mut level = 0;
        let mut next = Vec::new();
        println!("Creating tree nodes, start {start}, end {end}");
        while start < end - 1 {
            println!(
                "level: {}, packing {} nodes [{}]",
                level,
                search_nodes.len(),
                (end - start)
            );
            level += 1;
            search_nodes[start..end]
                .chunks(50)
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
                            _ => unreachable!("only leaf and tree nodes are allowed"),
                        }
                        bbox
                    });
                    next.push(SearchNode::TreeNode(TreeNode {
                        bbox,
                        index: start + (50 * index),
                    }));
                });
            start = end;
            end += next.len();
            search_nodes.extend(next.drain(..));
            next.clear();
        }

        println!(
            "Created {} search nodes, start: {start}, end: {end}",
            search_nodes.len()
        );
        if let SearchNode::TreeNode(tree) = &search_nodes.last().unwrap() {
            println!(
                "search bbox: {:?}, index: {}",
                geojson::Bbox::from(&tree.bbox),
                tree.index
            );
        } else {
            unreachable!("last entry of search nodes covers the whole tree")
        }

        RTree {
            leaf_nodes,
            search_nodes,
        }
    }

    pub fn nearest(&self, input_coordinate: &FPCoordinate) -> Option<(FPCoordinate, PartitionID)> {
        println!("searching for nearest neighbor: {:?}", input_coordinate);
        let root = *self.search_nodes.last().unwrap();
        let distance = match root {
            SearchNode::TreeNode(tree) => tree.bbox.min_distance(input_coordinate),
            _ => unreachable!("last entry of search nodes covers the whole tree"),
        };
        let mut nearest = None;
        let mut min_distance = f64::MAX;

        let mut queue = BinaryHeap::<QueueElement>::new();
        queue.push(QueueElement {
            node: root,
            distance,
        });

        while let Some(node) = queue.pop() {
            println!("popped node: {node:?} with distance {}", node.distance);
            match node.node {
                SearchNode::LeafNode(leaf) => {
                    println!(" searching leaf node: {leaf:?}");
                    self.leaf_nodes[leaf.index]
                        .elements
                        .iter()
                        .for_each(|candidate| {
                            let distance = input_coordinate.distance_to(&candidate.0);
                            println!("  candidate: {candidate:?} and distance {distance}");
                            queue.push(QueueElement {
                                node: SearchNode::Candidate(*candidate),
                                distance,
                            });
                        });
                }
                SearchNode::TreeNode(tree) => {
                    println!(" searching tree node: {tree:?}");
                    let children = &self.search_nodes
                        [tree.index..(tree.index + 50).min(self.search_nodes.len() - 1)];
                    for child in children {
                        println!("searching child: {child:?}");
                        match child {
                            SearchNode::LeafNode(leaf) => {
                                let distance = leaf.bbox.min_distance(input_coordinate);
                                if distance <= min_distance {
                                    queue.push(QueueElement {
                                        node: *child,
                                        distance,
                                    });
                                }
                            }
                            SearchNode::TreeNode(tree) => {
                                let distance = tree.bbox.min_distance(input_coordinate);
                                if distance <= min_distance {
                                    queue.push(QueueElement {
                                        node: *child,
                                        distance,
                                    });
                                }
                            }
                            _ => unreachable!("only leaf and tree nodes are allowed"),
                        }
                    }
                }
                SearchNode::Candidate((candidate_coordinate, candidate_id)) => {
                    println!(
                        " searching candidate: {candidate_coordinate:?} with id {candidate_id:?}"
                    );
                    let distance = input_coordinate.distance_to(&candidate_coordinate);
                    if distance < min_distance {
                        min_distance = distance;
                        nearest = Some((candidate_coordinate, candidate_id));
                        panic!("found nearest neighbor: {nearest:?}");
                    }
                }
            }
        }
        nearest
    }
}
