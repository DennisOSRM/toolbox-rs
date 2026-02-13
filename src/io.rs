use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read},
    path::Path,
};

use itertools::Itertools;
use rkyv::rancor;

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
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();

    let input_edges: Vec<InputEdge<usize>> =
        rkyv::from_bytes::<Vec<InputEdge<usize>>, rancor::Error>(&buf).unwrap();

    input_edges
        .iter()
        .map(|edge| TrivialEdge {
            source: edge.source,
            target: edge.target,
        })
        .collect_vec()
}

pub fn read_vec_from_file<T>(filename: &str) -> Vec<T>
where
    Vec<T>: rkyv::Archive,
    <Vec<T> as rkyv::Archive>::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rancor::Error>>
        + rkyv::Deserialize<Vec<T>, rancor::Strategy<rkyv::de::Pool, rancor::Error>>,
{
    let mut reader = BufReader::new(File::open(filename).unwrap());
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    rkyv::from_bytes::<Vec<T>, rancor::Error>(&buf).unwrap()
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
        // Define test input edges using the real InputEdge type
        let input_edges: Vec<InputEdge<usize>> = vec![
            InputEdge {
                source: 1,
                target: 2,
                data: 10,
            },
            InputEdge {
                source: 2,
                target: 3,
                data: 20,
            },
        ];

        // Serialize the input edges to a temporary file
        let mut file = NamedTempFile::new().unwrap();
        let bytes = rkyv::to_bytes::<rancor::Error>(&input_edges).unwrap();
        file.write_all(&bytes).unwrap();

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
        let test_data: Vec<i32> = vec![1, 2, 3, 4, 5];

        // Serialize the test data to a temporary file
        let mut file = NamedTempFile::new().unwrap();
        let bytes = rkyv::to_bytes::<rancor::Error>(&test_data).unwrap();
        file.write_all(&bytes).unwrap();

        // Read the vector from the file
        let result: Vec<i32> = read_vec_from_file(file.path().to_str().unwrap());

        // Verify the output
        assert_eq!(result, test_data);
    }

    // Test `read_vec_from_file` with a custom struct
    #[test]
    fn test_read_vec_from_file_with_custom_struct() {
        // Define a custom struct for testing
        #[derive(Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
        #[rkyv(compare(PartialEq), derive(Debug))]
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
        let bytes = rkyv::to_bytes::<rancor::Error>(&test_data).unwrap();
        file.write_all(&bytes).unwrap();

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
