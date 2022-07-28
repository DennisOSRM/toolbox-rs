use std::{
    fs::File,
    io::{self, BufRead},
    path::Path,
};

use itertools::Itertools;
use log::{debug, info};

use crate::{
    edge::{InputEdge, TrivialEdge},
    geometry::primitives::FPCoordinate,
    graph::NodeID,
};

pub enum WeightType {
    Unit,
    Original,
}

pub enum Direction {
    Both = 0,
    Forward = 1,
    Reverse = 2,
    Closed = 3,
}

impl TryFrom<i32> for Direction {
    type Error = ();

    fn try_from(v: i32) -> Result<Self, Self::Error> {
        match v {
            x if x == Direction::Both as i32 => Ok(Direction::Both),
            x if x == Direction::Forward as i32 => Ok(Direction::Forward),
            x if x == Direction::Reverse as i32 => Ok(Direction::Reverse),
            x if x == Direction::Closed as i32 => Ok(Direction::Closed),
            _ => Err(()),
        }
    }
}

// The output is wrapped in a Result to allow matching on errors
// Returns an Iterator to the Reader of the lines of the file.
fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

pub fn read_graph<T: std::cmp::Eq + From<i32>>(
    filename: &str,
    _weight_type: WeightType,
) -> Vec<InputEdge<T>> {
    let mut edges = Vec::new();

    let mut lines = read_lines(filename).expect("could not load graph file");

    let first_line = lines.next().unwrap().unwrap();
    let sizes = first_line
        .get(..)
        .unwrap_or("")
        .split_ascii_whitespace()
        .collect_vec();
    info!("expecting {} nodes and {} edges", sizes[0], sizes[1]);

    let mut current_source: NodeID = 0;

    // load unweighted metis graph and coordinates
    for line in lines {
        let line = line.unwrap();
        let tokens = line.get(..).unwrap_or("").split_whitespace().collect_vec();

        for token in tokens {
            edges.push(InputEdge {
                source: current_source,
                target: token.parse::<NodeID>().unwrap() - 1,
                data: T::from(1),
            });
        }
        current_source += 1;
    }
    info!("loaded {} directed edges", edges.len());
    edges
}

pub fn read_graph_into_trivial_edges(filename: &str) -> Vec<TrivialEdge> {
    let mut comment_count = 0;
    let mut problem_count = 0;
    let mut edges = Vec::new();

    // load dimacs graph and coordinates
    for line in read_lines(filename).expect("could not load graph file") {
        let line = line.unwrap();
        match line.chars().next().unwrap() {
            'c' => {
                comment_count += 1;
                debug!("{}", line.get(2..).unwrap_or(""));
            }
            'p' => {
                problem_count += 1;
                let sizes = line
                    .get(5..)
                    .unwrap_or("")
                    .split_ascii_whitespace()
                    .collect_vec();
                info!("expecting {} nodes and {} edges", sizes[0], sizes[1]);
                edges.reserve(sizes[0].parse::<usize>().unwrap());
            }
            'a' => {
                let tokens = line.get(2..).unwrap_or("").split_whitespace().collect_vec();
                if tokens.len() != 3 {
                    continue;
                }
                let source = tokens[0].parse::<NodeID>().unwrap();
                let target = tokens[1].parse::<NodeID>().unwrap();
                edges.push(TrivialEdge { source, target });
            }
            _ => {}
        }
    }
    debug!("graph file comment count: {comment_count}");
    debug!("graph file problem count: {problem_count}");

    info!("renumbering source and target in edge list");
    for mut edge in &mut edges {
        // the DIMACS format defines numbering to be consecutive and starting at 1.
        edge.source -= 1;
        edge.target -= 1;
    }

    edges
}

pub fn read_coordinates(filename: &str) -> Vec<FPCoordinate> {
    let mut coordinates = Vec::new();
    for line in read_lines(filename).expect("could not load coordinates file") {
        let line = line.unwrap();
        let tokens = line.get(..).unwrap_or("").split_whitespace().collect_vec();
        let lon = tokens[0].parse::<f64>().unwrap() / 100_000.;
        let lat = tokens[1].parse::<f64>().unwrap() / 100_000.;
        // let _z = tokens[2].parse::<f64>().unwrap();
        coordinates.push(FPCoordinate::new_from_lat_lon(lat, lon));
    }

    coordinates
}
