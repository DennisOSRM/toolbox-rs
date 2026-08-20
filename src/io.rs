use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Write},
    path::Path,
};

use itertools::Itertools;
use rkyv::rancor;

use crate::{
    edge::{InputEdge, StoredEdge, TrivialEdge},
    graph::NodeID,
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

/// Reads a single value that was written by rkyv, as opposed to a list of
/// them.
///
/// # Panics
///
/// Panics if the file cannot be read or does not hold what was asked for.
pub fn read_from_file<T>(filename: &str) -> T
where
    T: rkyv::Archive,
    <T as rkyv::Archive>::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rancor::Error>>
        + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rancor::Error>>,
{
    let mut reader = BufReader::new(File::open(filename).unwrap());
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf).unwrap();
    rkyv::from_bytes::<T, rancor::Error>(&buf).unwrap()
}

/// Reads a graph's arcs, narrowing what each costs to four bytes.
///
/// What is on disk holds a cost of eight bytes, which is what the crate wrote
/// before the adjacency array was narrowed. Reading it as it lies and narrowing
/// here keeps every instance already written readable, and the wide form never
/// reaches the array a search walks.
///
/// # Panics
///
/// Panics if the file cannot be read, or holds a cost too wide for four bytes.
#[must_use]
pub fn read_edges_from_file(filename: &str) -> Vec<InputEdge<u32>> {
    read_vec_from_file::<StoredEdge<usize>>(filename)
        .into_iter()
        .map(|edge| {
            InputEdge::new(
                NodeID::try_from(edge.source).expect("the graph is too large to hold"),
                NodeID::try_from(edge.target).expect("the graph is too large to hold"),
                u32::try_from(edge.data).expect("an arc costing more than four bytes reach"),
            )
        })
        .collect()
}

/// Writes a value for [`read_from_file`] to read back.
///
/// # Panics
///
/// Panics if the file cannot be written or the value cannot be laid out.
pub fn write_to_file<T>(filename: &str, value: &T)
where
    T: for<'a> rkyv::Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rancor::Error,
            >,
        >,
{
    let bytes = rkyv::to_bytes::<rancor::Error>(value).unwrap();
    let mut file = std::io::BufWriter::new(File::create(filename).unwrap());
    file.write_all(&bytes).unwrap();
    file.flush().unwrap();
}

/// Writes a list for [`read_vec_from_file`] to read back.
///
/// # Panics
///
/// Panics if the file cannot be written or the list cannot be laid out.
pub fn write_vec_to_file<T>(filename: &str, values: &Vec<T>)
where
    Vec<T>: for<'a> rkyv::Serialize<
            rkyv::api::high::HighSerializer<
                rkyv::util::AlignedVec,
                rkyv::ser::allocator::ArenaHandle<'a>,
                rancor::Error,
            >,
        >,
{
    write_to_file(filename, values);
}

#[cfg(test)]
mod tests {
    /// What was written has to read back as what it was, or an instance
    /// written out by one step is not the instance the next one reads.
    #[test]
    fn a_list_reads_back_as_it_was_written() {
        use crate::edge::InputEdge;
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap();
        let written = vec![
            InputEdge::new(0, 1, 7_usize),
            InputEdge::new(1, 2, 11),
            InputEdge::new(2, 0, 3),
        ];
        super::write_vec_to_file(path, &written);
        let read = super::read_vec_from_file::<InputEdge<usize>>(path);
        assert_eq!(read, written);
    }

    #[test]
    fn a_value_reads_back_as_it_was_written() {
        use crate::level_directory::LevelDirectory;
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_str().unwrap();
        let written = LevelDirectory::new(vec![0, 0, 1, 1], vec![vec![0, 0]]);
        super::write_to_file(path, &written);
        let read: LevelDirectory = super::read_from_file(path);
        assert_eq!(read, written);
    }

    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn a_single_value_survives_the_file() {
        use crate::geometry::FPCoordinate;
        let coordinate = FPCoordinate::new(1_234_567, -7_654_321);

        let mut file = NamedTempFile::new().unwrap();
        let bytes = rkyv::to_bytes::<rancor::Error>(&coordinate).unwrap();
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();

        let path = file.path().to_str().unwrap();
        assert_eq!(read_from_file::<FPCoordinate>(path), coordinate);
    }

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
