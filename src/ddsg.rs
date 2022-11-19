use itertools::Itertools;
use log::info;

use crate::{edge::InputEdge, geometry::primitives::FPCoordinate, graph::NodeID, io::read_lines};

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

pub fn read_graph<T: std::cmp::Eq + From<usize>>(
    filename: &str,
    weight_type: WeightType,
) -> Vec<InputEdge<T>> {
    let mut edges = Vec::new();

    let mut lines = read_lines(filename).expect("could not load graph file");

    let first_line = lines.next().unwrap().unwrap();
    if first_line != "d" {
        return edges;
    }

    let second_line = lines.next().unwrap().unwrap();
    let sizes = second_line
        .get(..)
        .unwrap_or("")
        .split_ascii_whitespace()
        .collect_vec();
    info!("expecting {} nodes and {} edges", sizes[0], sizes[1]);

    let mut input_edge_counter = 0;

    // load dimacs graph and coordinates
    for line in lines {
        let line = line.unwrap();
        // IDSTARTNODE IDDESTNODE WEIGHT BLOCKEDDIR DISTANCE TIME
        let tokens = line.get(..).unwrap_or("").split_whitespace().collect_vec();
        if tokens.len() != 4 {
            continue;
        }
        let source = tokens[0].parse::<NodeID>().unwrap();
        let target = tokens[1].parse::<NodeID>().unwrap();

        // avoid eigenloops
        if source == target {
            continue;
        }

        let data = tokens[2].parse::<usize>().unwrap();
        let direction = Direction::try_from(tokens[3].parse::<i32>().unwrap()).unwrap();
        input_edge_counter += 1;

        match direction {
            Direction::Both => {
                edges.push(InputEdge::<T> {
                    source,
                    target,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
                edges.push(InputEdge::<T> {
                    target,
                    source,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
            }
            Direction::Forward => {
                edges.push(InputEdge::<T> {
                    source,
                    target,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
            }
            Direction::Reverse => {
                edges.push(InputEdge::<T> {
                    target,
                    source,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
            }
            Direction::Closed => {
                // closed in both directions, thus ignore
            }
        }
    }
    info!(
        "exploded {input_edge_counter} input edges into {} directed edges",
        edges.len()
    );
    edges
}

pub fn read_coordinates(filename: &str) -> Vec<FPCoordinate> {
    let mut lines = read_lines(filename).expect("could not load coordinates file");
    let first_line = lines.next().unwrap().unwrap();
    let coordinate_count = first_line.parse::<usize>().unwrap();
    info!("expecting {coordinate_count} coordinates");
    let mut coordinates = Vec::with_capacity(coordinate_count);
    for line in lines {
        let line = line.unwrap();
        let tokens = line.get(..).unwrap_or("").split_whitespace().collect_vec();
        let count = tokens[0].parse::<usize>().unwrap();
        assert_eq!(count, coordinates.len());

        let lon = tokens[1].parse::<f64>().unwrap() / 100_000.;
        let lat = tokens[2].parse::<f64>().unwrap() / 100_000.;
        coordinates.push(FPCoordinate::new_from_lat_lon(lat, lon));
    }
    assert_eq!(coordinate_count, coordinates.len());
    info!("loaded {coordinate_count} coordinates");

    coordinates
}
