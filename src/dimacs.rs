use itertools::Itertools;
use log::{debug, info};

use crate::{edge::InputEdge, geometry::primitives::FPCoordinate, graph::NodeID, io::read_lines};

pub enum WeightType {
    Unit,
    Original,
}

pub fn read_graph<T: std::cmp::Eq + From<usize>>(
    filename: &str,
    weight_type: WeightType,
) -> Vec<InputEdge<T>> {
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
                // avoid eigenloops
                if source == target {
                    continue;
                }
                let data = tokens[2].parse::<usize>().unwrap();

                edges.push(InputEdge::<T> {
                    source,
                    target,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
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
    let mut comment_count = 0;
    let mut problem_count = 0;
    for line in read_lines(filename).expect("could not load coordinates file") {
        let line = line.unwrap();
        match line.chars().next().unwrap() {
            'c' => {
                comment_count += 1;
                debug!("{}", line.get(2..).unwrap_or(""));
            }
            'p' => {
                problem_count += 1;
                let size = line.get(12..).unwrap_or("").parse::<usize>().unwrap();
                info!("expecting {size} coordinates");
                coordinates.reserve(size);
            }
            'v' => {
                let tokens = line.get(2..).unwrap_or("").split_whitespace().collect_vec();
                let id = tokens[0].parse::<NodeID>().unwrap();
                let lon = tokens[1].parse::<i32>().unwrap();
                let lat = tokens[2].parse::<i32>().unwrap();
                coordinates.push(FPCoordinate::new(lat, lon));
                debug_assert!(coordinates.len() == id);
            }
            _ => {}
        }
    }
    debug!("coordinates file comment count: {comment_count}");
    debug!("coordinates file problem count: {problem_count}");

    coordinates
}
