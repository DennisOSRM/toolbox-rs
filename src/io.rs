use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};

use bincode::deserialize_from;
use itertools::Itertools;

use crate::{
    edge::{InputEdge, TrivialEdge},
    geometry::primitives::FPCoordinate,
};

// The output is wrapped in a Result to allow matching on errors
// Returns an Iterator to the Reader of the lines of the file.
pub fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

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
