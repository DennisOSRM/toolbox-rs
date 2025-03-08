use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};

use bincode::{config, decode_from_std_read};
use itertools::Itertools;

use crate::edge::{InputEdge, TrivialEdge};

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
    let mut reader = BufReader::new(File::open(filename).unwrap());
    let config = config::standard();

    let input_edges: Vec<InputEdge<usize>> = decode_from_std_read(&mut reader, config).unwrap();
    let edges = input_edges
        .iter()
        .map(|edge| TrivialEdge {
            source: edge.source,
            target: edge.target,
        })
        .collect_vec();

    edges
}

pub fn read_vec_from_file<T: bincode::Decode<()>>(filename: &str) -> Vec<T> {
    let mut reader = BufReader::new(File::open(filename).unwrap());
    let config = config::standard();
    decode_from_std_read(&mut reader, config).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Test `read_lines` function
    #[test]
    fn test_read_lines() {
        // Create a temporary file with some lines
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line1").unwrap();
        writeln!(file, "line2").unwrap();
        writeln!(file, "line3").unwrap();

        // Read lines from the file
        let lines = read_lines(file.path()).unwrap();
        let lines: Vec<String> = lines.map(|line| line.unwrap()).collect();

        // Verify the lines are read correctly
        assert_eq!(lines, vec!["line1", "line2", "line3"]);
    }

    // Test `read_lines` with a non-existent file
    #[test]
    fn test_read_lines_nonexistent_file() {
        let result = read_lines("nonexistent_file.txt");
        assert!(result.is_err());
    }

    // Test `read_graph_into_trivial_edges` function
    #[test]
    fn test_read_graph_into_trivial_edges() {
        // Define test input edges
        #[derive(bincode::Encode)]
        struct TestEdge {
            source: usize,
            target: usize,
            weight: usize,
        }

        let input_edges = vec![
            TestEdge {
                source: 1,
                target: 2,
                weight: 10,
            },
            TestEdge {
                source: 2,
                target: 3,
                weight: 20,
            },
        ];

        // Serialize the input edges to a temporary file
        let mut file = NamedTempFile::new().unwrap();
        let config = config::standard();
        bincode::encode_into_std_write(&input_edges, &mut file, config).unwrap();

        // Read the graph into trivial edges
        let trivial_edges = read_graph_into_trivial_edges(file.path().to_str().unwrap());

        // Verify the output
        assert_eq!(trivial_edges.len(), 2);
        assert_eq!(trivial_edges[0].source, 1);
        assert_eq!(trivial_edges[0].target, 2);
        assert_eq!(trivial_edges[1].source, 2);
        assert_eq!(trivial_edges[1].target, 3);
    }

    // Test `read_graph_into_trivial_edges` with a non-existent file
    #[test]
    #[should_panic]
    fn test_read_graph_into_trivial_edges_nonexistent_file() {
        read_graph_into_trivial_edges("nonexistent_file.bin");
    }

    // Test `read_vec_from_file` function
    #[test]
    fn test_read_vec_from_file() {
        // Define test data
        let test_data = vec![1, 2, 3, 4, 5];

        // Serialize the test data to a temporary file
        let mut file = NamedTempFile::new().unwrap();
        let config = config::standard();
        bincode::encode_into_std_write(&test_data, &mut file, config).unwrap();

        // Read the vector from the file
        let result: Vec<i32> = read_vec_from_file(file.path().to_str().unwrap());

        // Verify the output
        assert_eq!(result, test_data);
    }

    // Test `read_vec_from_file` with a custom struct
    #[test]
    fn test_read_vec_from_file_with_custom_struct() {
        // Define a custom struct for testing
        #[derive(Debug, PartialEq, bincode::Encode, bincode::Decode)]
        struct TestStruct {
            id: u64,
            name: String,
        }

        let test_data = vec![
            TestStruct {
                id: 1,
                name: "Alice".to_string(),
            },
            TestStruct {
                id: 2,
                name: "Bob".to_string(),
            },
        ];

        // Serialize the test data to a temporary file
        let mut file = NamedTempFile::new().unwrap();
        let config = config::standard();
        bincode::encode_into_std_write(&test_data, &mut file, config).unwrap();

        // Read the vector from the file
        let result: Vec<TestStruct> = read_vec_from_file(file.path().to_str().unwrap());

        // Verify the output
        assert_eq!(result, test_data);
    }

    // Test `read_vec_from_file` with a non-existent file
    #[test]
    #[should_panic]
    fn test_read_vec_from_file_nonexistent_file() {
        read_vec_from_file::<i32>("nonexistent_file.bin");
    }

    // Test `read_vec_from_file` with invalid data
    #[test]
    #[should_panic]
    fn test_read_vec_from_file_invalid_data() {
        // Create a temporary file with invalid binary data
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"invalid binary data").unwrap();

        // Attempt to read the invalid data
        let _: Vec<i32> = read_vec_from_file(file.path().to_str().unwrap());
    }
}
