use itertools::Itertools;
use log::info;

use crate::{edge::InputEdge, geometry::primitives::FPCoordinate, graph::NodeID, io::read_lines};

pub enum WeightType {
    Unit,
    Original,
}

#[derive(Debug, PartialEq)]
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

pub fn read_graph<T: std::fmt::Debug + std::cmp::Eq + From<usize>>(
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
        let u = tokens[0].parse::<NodeID>().unwrap();
        let v = tokens[1].parse::<NodeID>().unwrap();

        // avoid eigenloops
        if u == v {
            continue;
        }

        let data = tokens[2].parse::<usize>().unwrap();
        let direction = Direction::try_from(tokens[3].parse::<i32>().unwrap()).unwrap();
        input_edge_counter += 1;

        match direction {
            Direction::Both => {
                edges.push(InputEdge::<T> {
                    source: u,
                    target: v,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
                edges.push(InputEdge::<T> {
                    source: v,
                    target: u,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
            }
            Direction::Forward => {
                edges.push(InputEdge::<T> {
                    source: u,
                    target: v,
                    data: match &weight_type {
                        WeightType::Unit => T::from(1),
                        WeightType::Original => T::from(data),
                    },
                });
            }
            Direction::Reverse => {
                edges.push(InputEdge::<T> {
                    source: v,
                    target: u,
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use tempfile::NamedTempFile;

    use crate::ddsg::{read_coordinates, Direction, WeightType};

    #[test]
    fn direction_try_from() {
        assert_eq!(Direction::try_from(0), Ok(Direction::Both));
        assert_eq!(Direction::try_from(1), Ok(Direction::Forward));
        assert_eq!(Direction::try_from(2), Ok(Direction::Reverse));
        assert_eq!(Direction::try_from(3), Ok(Direction::Closed));
        assert_eq!(Direction::try_from(4), Err(()));
    }

    #[test]
    fn read_graph_undirected() {
        let filename = create_temp_file_with_content("d\n2 1\n0 1 10 0\n");
        let edges = crate::ddsg::read_graph::<usize>(
            filename.path().to_str().unwrap(),
            WeightType::Original,
        );
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 10);
        assert_eq!(edges[1].source, 1);
        assert_eq!(edges[1].target, 0);
        assert_eq!(edges[1].data, 10);
    }

    #[test]
    fn read_graph_with_invalid_data() {
        let filename = create_temp_file_with_content("t\n2 1\n0 1 10 0\n");
        let edges = crate::ddsg::read_graph::<usize>(
            filename.path().to_str().unwrap(),
            WeightType::Original,
        );
        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn read_graph_with_different_directions() {
        let filename = create_temp_file_with_content("d\n3 3\n0 1 10 1\n1 2 20 2\n2 0 30 3\n");
        let edges = crate::ddsg::read_graph::<usize>(
            filename.path().to_str().unwrap(),
            WeightType::Original,
        );
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 10);
        assert_eq!(edges[1].source, 2);
        assert_eq!(edges[1].target, 1);
        assert_eq!(edges[1].data, 20);
    }

    #[test]
    fn read_ddsg_coordinates() {
        let filename = create_temp_file_with_content("2\n0 1000000 2000000\n1 3000000 4000000\n");
        let coordinates = crate::ddsg::read_coordinates(filename.path().to_str().unwrap());
        assert_eq!(coordinates.len(), 2);
        assert_eq!(coordinates[0].to_lon_lat_pair(), (10., 20.));
        assert_eq!(coordinates[1].to_lon_lat_pair(), (30., 40.));
    }

    #[test]
    fn read_coordinates_with_invalid_data() {
        let filename =
            create_temp_file_with_content("2\n0 1000000 2000000\n1 invalid_data 4000000\n");
        let result =
            std::panic::catch_unwind(|| read_coordinates(filename.path().to_str().unwrap()));
        assert!(result.is_err());
    }

    fn create_temp_file_with_content(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        file
    }
}
