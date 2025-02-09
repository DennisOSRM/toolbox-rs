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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::write;
    use tempfile::NamedTempFile;

    #[test]
    fn read_graph_valid() {
        let graph_content = "4 8\n3 2\n2 3\n1 3\n1 2\n";
        let tmp_file = NamedTempFile::new().unwrap();
        write(tmp_file.path(), graph_content).unwrap();

        let edges = read_graph::<usize>(tmp_file.path().to_str().unwrap(), WeightType::Unit);

        assert_eq!(edges.len(), 6);
        assert_eq!(
            edges[0],
            InputEdge {
                source: 0,
                target: 2,
                data: 1
            }
        );
        assert_eq!(
            edges[1],
            InputEdge {
                source: 0,
                target: 1,
                data: 1
            }
        );
        assert_eq!(
            edges[2],
            InputEdge {
                source: 1,
                target: 2,
                data: 1
            }
        );
        assert_eq!(
            edges[3],
            InputEdge {
                source: 2,
                target: 0,
                data: 1
            }
        );
        assert_eq!(
            edges[4],
            InputEdge {
                source: 3,
                target: 0,
                data: 1
            }
        );
        assert_eq!(
            edges[5],
            InputEdge {
                source: 3,
                target: 1,
                data: 1
            }
        );
    }

    #[test]
    fn read_coordinates_valid() {
        let coord_content = "1234567 4567890\n2345678 5678901\n";
        let tmp_file = NamedTempFile::new().unwrap();
        write(tmp_file.path(), coord_content).unwrap();

        let coords = read_coordinates(tmp_file.path().to_str().unwrap());

        assert_eq!(coords.len(), 2);
        let (lon, lat) = coords[0].to_lon_lat_pair();
        assert!((lat - 45.67890).abs() < 1e-5);
        assert!((lon - 12.34567).abs() < 1e-5);
    }

    #[test]
    #[should_panic]
    fn read_graph_invalid_file() {
        read_graph::<usize>("nonexistent_file.txt", WeightType::Unit);
    }

    #[test]
    #[should_panic]
    fn read_graph_invalid_format() {
        let graph_content = "not a number\n1 2\n";
        let tmp_file = NamedTempFile::new().unwrap();
        write(tmp_file.path(), graph_content).unwrap();

        read_graph::<usize>(tmp_file.path().to_str().unwrap(), WeightType::Unit);
    }

    #[test]
    #[should_panic]
    fn read_graph_node_out_of_bounds() {
        let graph_content = "2 1\n3\n1\n"; // Node 3 exceeds number_of_nodes
        let tmp_file = NamedTempFile::new().unwrap();
        write(tmp_file.path(), graph_content).unwrap();

        read_graph::<usize>(tmp_file.path().to_str().unwrap(), WeightType::Unit);
    }

    #[test]
    fn read_graph_skip_eigenloops() {
        let graph_content = "2 1\n1 1\n2\n";
        let tmp_file = NamedTempFile::new().unwrap();
        write(tmp_file.path(), graph_content).unwrap();

        let edges = read_graph::<usize>(tmp_file.path().to_str().unwrap(), WeightType::Unit);

        assert_eq!(edges.len(), 0); // Eigenloop should be skipped
    }

    #[test]
    fn direction_try_from() {
        // test valid input values
        assert!(matches!(Direction::try_from(0), Ok(Direction::Both)));
        assert!(matches!(Direction::try_from(1), Ok(Direction::Forward)));
        assert!(matches!(Direction::try_from(2), Ok(Direction::Reverse)));
        assert!(matches!(Direction::try_from(3), Ok(Direction::Closed)));

        // test invalid input values
        assert!(Direction::try_from(-1).is_err());
        assert!(Direction::try_from(4).is_err());
    }

    #[test]
    fn direction_as_i32() {
        assert_eq!(Direction::Both as i32, 0);
        assert_eq!(Direction::Forward as i32, 1);
        assert_eq!(Direction::Reverse as i32, 2);
        assert_eq!(Direction::Closed as i32, 3);
    }
}
