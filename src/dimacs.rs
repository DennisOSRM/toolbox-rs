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
                let sizes = &line.split_ascii_whitespace().collect_vec()[2..4];
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
    for edge in &mut edges {
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
                debug_assert!(line.starts_with("p aux sp co"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn test_read_graph_unit_weights() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("graph.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "p sp 4 5").unwrap();
        writeln!(file, "a 1 2 1").unwrap();
        writeln!(file, "a 2 3 1").unwrap();
        writeln!(file, "a 3 4 1").unwrap();
        writeln!(file, "a 4 1 1").unwrap();
        writeln!(file, "a 1 3 1").unwrap();

        let edges = read_graph::<NodeID>(file_path.to_str().unwrap(), WeightType::Unit);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 1);
        assert_eq!(edges[4].source, 0);
        assert_eq!(edges[4].target, 2);
        assert_eq!(edges[4].data, 1);
    }

    #[test]
    fn test_read_graph_unit_weights_eigenloops() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("graph.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "p sp 4 5").unwrap();
        writeln!(file, "a 1 2 1").unwrap();
        writeln!(file, "a 2 3 1").unwrap();
        writeln!(file, "a 3 4 1").unwrap();
        writeln!(file, "a 4 1 1").unwrap();
        writeln!(file, "a 1 3 1").unwrap();
        writeln!(file, "a 1 1 1").unwrap();

        let edges = read_graph::<NodeID>(file_path.to_str().unwrap(), WeightType::Unit);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 1);
        assert_eq!(edges[4].source, 0);
        assert_eq!(edges[4].target, 2);
        assert_eq!(edges[4].data, 1);
    }

    #[test]
    fn test_read_graph_unit_weights_broken_lines() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("graph.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "p sp 4 5").unwrap();
        writeln!(file, "a 1 2 1").unwrap();
        writeln!(file, "a 2 3 1").unwrap();
        writeln!(file, "a 3 4 1").unwrap();
        writeln!(file, "a 1 1 1 6").unwrap();
        writeln!(file, "a 4 1 1").unwrap();
        writeln!(file, "x 4 1 1").unwrap();
        writeln!(file, "h 4 1 1").unwrap();
        writeln!(file, "a 1 3 1").unwrap();

        let edges = read_graph::<NodeID>(file_path.to_str().unwrap(), WeightType::Unit);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 1);
        assert_eq!(edges[4].source, 0);
        assert_eq!(edges[4].target, 2);
        assert_eq!(edges[4].data, 1);
    }

    #[test]
    fn test_read_graph_original_weights() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("graph.txt");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "p sp 4 5").unwrap();
        writeln!(file, "a 1 2 10").unwrap();
        writeln!(file, "a 2 3 20").unwrap();
        writeln!(file, "a 3 4 30").unwrap();
        writeln!(file, "a 4 1 40").unwrap();
        writeln!(file, "a 1 3 50").unwrap();

        let edges = read_graph::<NodeID>(file_path.to_str().unwrap(), WeightType::Original);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[0].source, 0);
        assert_eq!(edges[0].target, 1);
        assert_eq!(edges[0].data, 10);
        assert_eq!(edges.len(), 5);
        assert_eq!(edges[4].source, 0);
        assert_eq!(edges[4].target, 2);
        assert_eq!(edges[4].data, 50);
    }

    #[test]
    fn test_read_graph_invalid_file() {
        let result = std::panic::catch_unwind(|| {
            read_graph::<NodeID>("invalid_file.txt", WeightType::Unit);
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_read_coordinates_valid_file() {
        // Create a temporary file with valid input
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "p aux sp co 3").unwrap();
        writeln!(file, "v 1 100 200").unwrap();
        writeln!(file, "v 2 150 250").unwrap();
        writeln!(file, "v 3 300 400").unwrap();

        // Call the function
        let coordinates = read_coordinates(file.path().to_str().unwrap());

        // Verify the results
        assert_eq!(coordinates.len(), 3);
        assert_eq!(coordinates[0], FPCoordinate::new(200, 100));
        assert_eq!(coordinates[1], FPCoordinate::new(250, 150));
        assert_eq!(coordinates[2], FPCoordinate::new(400, 300));
    }

    #[test]
    fn test_read_coordinates_empty_file() {
        // Create an empty temporary file
        let file = NamedTempFile::new().unwrap();

        // Call the function
        let coordinates = read_coordinates(file.path().to_str().unwrap());

        // Verify the results
        assert_eq!(coordinates.len(), 0);
    }

    #[test]
    fn test_read_coordinates_invalid_line_format() {
        // Create a temporary file with invalid input
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "v invalid 100 200").unwrap(); // Invalid NodeID

        // Call the function and expect a panic
        let result = std::panic::catch_unwind(|| {
            read_coordinates(file.path().to_str().unwrap());
        });

        // Verify that the function panicked
        assert!(result.is_err());
    }

    #[test]
    fn test_read_coordinates_missing_problem_line() {
        // Create a temporary file without a problem line
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "c This is a comment").unwrap();
        writeln!(file, "v 1 100 200").unwrap();

        // Call the function
        let coordinates = read_coordinates(file.path().to_str().unwrap());

        // Verify the results
        assert_eq!(coordinates.len(), 1);
        assert_eq!(coordinates[0], FPCoordinate::new(200, 100));
    }

    #[test]
    fn test_read_coordinates_large_file() {
        // Create a temporary file with a large number of coordinates
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "p aux sp co 1000").unwrap();
        for i in 1..=1000 {
            writeln!(file, "v {} {} {}", i, i * 100, i * 200).unwrap();
        }

        // Call the function
        let coordinates = read_coordinates(file.path().to_str().unwrap());

        // Verify the results
        assert_eq!(coordinates.len(), 1000);
        for i in 0..1000 {
            assert_eq!(
                coordinates[i as usize],
                FPCoordinate::new((i + 1) * 200, (i + 1) * 100)
            );
        }
    }

    #[test]
    fn test_read_coordinates_invalid_latitude_longitude() {
        // Create a temporary file with invalid latitude/longitude values
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "v 1 invalid 200").unwrap(); // Invalid longitude

        // Call the function and expect a panic
        let result = std::panic::catch_unwind(|| {
            read_coordinates(file.path().to_str().unwrap());
        });

        // Verify that the function panicked
        assert!(result.is_err());
    }
}
