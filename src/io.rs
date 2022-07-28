use std::{fs::File, io::BufReader};

use bincode::deserialize_from;
use itertools::Itertools;

use crate::{
    edge::{InputEdge, TrivialEdge},
    geometry::primitives::FPCoordinate,
};

pub fn read_graph_into_trivial_edges(filename: &str) -> Vec<TrivialEdge> {
    let reader = BufReader::new(File::open(filename).unwrap());

    let input_edges: Vec<InputEdge<i32>> = deserialize_from(reader).unwrap();
    let edges = input_edges
        .iter()
        .map(|edge| TrivialEdge {
            source: edge.source,
            target: edge.target,
        })
        .collect_vec();

    edges
}

pub fn read_coordinates(filename: &str) -> Vec<FPCoordinate> {
    let reader = BufReader::new(File::open(filename).unwrap());
    deserialize_from(reader).unwrap()
}
