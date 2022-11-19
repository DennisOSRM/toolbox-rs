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
    let number_of_nodes = sizes[0].parse::<NodeID>().unwrap();
    info!("expecting {} nodes and {} edges", number_of_nodes, sizes[1]);

    // load unweighted metis graph and coordinates
    for (source, line) in lines.enumerate() {
        let line = line.unwrap();
        let tokens = line.get(..).unwrap_or("").split_whitespace().collect_vec();

        assert!(source < number_of_nodes);

        for token in tokens {
            let target = token.parse::<NodeID>().unwrap() - 1;
            assert!(target < number_of_nodes);
            // avoid eigenloops
            if source == target {
                continue;
            }

            edges.push(InputEdge {
                source,
                target,
                data: T::from(1),
            });
        }
    }
    info!("loaded {} directed edges", edges.len());
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
