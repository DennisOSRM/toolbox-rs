use crate::{geometry::IPoint2D, io::read_lines};
use std::str::FromStr;

/// A site in a TSP problem, consisting of an ID and integer coordinates
#[derive(Debug, Clone)]
pub struct TspSite {
    pub id: usize,
    pub coordinate: IPoint2D,
}

#[derive(Debug)]
pub enum TspError {
    IoError(std::io::Error),
    ParseError(String),
}

impl From<std::io::Error> for TspError {
    fn from(error: std::io::Error) -> Self {
        TspError::IoError(error)
    }
}

/// Parse a TSP file containing site coordinates.
///
/// Format specification:
/// - Header section with metadata (NAME, COMMENT, TYPE, DIMENSION, EDGE_WEIGHT_TYPE)
/// - NODE_COORD_SECTION marker
/// - List of nodes with format: <id> <x> <y>
///
/// # Arguments
/// * `filename` - Path to the TSP file
///
/// # Returns
/// A vector of TspSite objects containing the parsed coordinates
pub fn read_tsp_file(filename: &str) -> Result<Vec<TspSite>, TspError> {
    let mut sites = Vec::new();
    let mut in_coord_section = false;
    let mut dimension: Option<usize> = None;

    for line in read_lines(filename)? {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line == "EOF" {
            continue;
        }

        if line == "NODE_COORD_SECTION" {
            in_coord_section = true;
            continue;
        }

        if !in_coord_section {
            // Parse header section
            if line.starts_with("DIMENSION") {
                if let Some(dim_str) = line.split(':').nth(1) {
                    dimension = Some(dim_str.trim().parse().map_err(|_| {
                        TspError::ParseError("Invalid DIMENSION value".to_string())
                    })?);
                }
            }
            continue;
        }

        // Parse coordinate section
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(TspError::ParseError(format!(
                "Invalid coordinate line: {}",
                line
            )));
        }

        let id = parts[0]
            .parse()
            .map_err(|_| TspError::ParseError(format!("Invalid id: {}", parts[0])))?;
        let x = f64::from_str(parts[1])
            .map_err(|_| TspError::ParseError(format!("Invalid x coordinate: {}", parts[1])))?
            as i32;
        let y = f64::from_str(parts[2])
            .map_err(|_| TspError::ParseError(format!("Invalid y coordinate: {}", parts[2])))?
            as i32;

        sites.push(TspSite {
            id,
            coordinate: IPoint2D::new(x, y),
        });
    }

    // Verify we read the expected number of sites
    if let Some(dim) = dimension {
        if sites.len() != dim {
            return Err(TspError::ParseError(format!(
                "Expected {} sites but found {}",
                dim,
                sites.len()
            )));
        }
    }

    Ok(sites)
}

/// Calculate the Euclidean distance between two TSP sites
pub fn euclidean_distance(a: &TspSite, b: &TspSite) -> i32 {
    a.coordinate.distance_to(&b.coordinate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind, Write};
    use tempfile::NamedTempFile;

    fn create_test_tsp_file() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : test_problem
COMMENT : A small 4-city TSP instance for testing
TYPE : TSP
DIMENSION : 4
EDGE_WEIGHT_TYPE : EUC_2D
NODE_COORD_SECTION
1 0 0
2 3 0
3 0 4
4 3 4
EOF"
        )
        .unwrap();
        file
    }

    #[test]
    fn test_euclidean_distance() {
        let site1 = TspSite {
            id: 1,
            coordinate: IPoint2D::new(0, 0),
        };
        let site2 = TspSite {
            id: 2,
            coordinate: IPoint2D::new(3, 4),
        };

        let distance = euclidean_distance(&site1, &site2);
        assert_eq!(distance, 5);
    }

    #[test]
    fn test_parse_tsp_file() {
        let file = create_test_tsp_file();
        let sites = read_tsp_file(file.path().to_str().unwrap()).unwrap();

        assert_eq!(sites.len(), 4, "Should parse exactly 4 sites");

        // Check first site coordinates
        assert_eq!(sites[0].coordinate.x, 0);
        assert_eq!(sites[0].coordinate.y, 0);
        assert_eq!(sites[0].id, 1);

        // Verify distances
        let d12 = euclidean_distance(&sites[0], &sites[1]); // horizontal distance
        let d13 = euclidean_distance(&sites[0], &sites[2]); // vertical distance
        let d14 = euclidean_distance(&sites[0], &sites[3]); // diagonal distance

        assert_eq!(d12, 3, "Distance between sites 1 and 2 should be 3");
        assert_eq!(d13, 4, "Distance between sites 1 and 3 should be 4");
        assert_eq!(d14, 5, "Distance between sites 1 and 4 should be 5");
    }

    #[test]
    fn test_parse_invalid_dimension() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : invalid_problem
DIMENSION : not_a_number
NODE_COORD_SECTION
1 0 0
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(matches!(result, Err(TspError::ParseError(_))));
    }

    #[test]
    fn test_parse_wrong_dimension() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : wrong_dimension
DIMENSION : 2
NODE_COORD_SECTION
1 0 0
2 1 1
3 2 2
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(matches!(result, Err(TspError::ParseError(_))));
    }

    #[test]
    fn test_parse_invalid_coordinates() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : invalid_coords
DIMENSION : 1
NODE_COORD_SECTION
1 not_a_number 0
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(matches!(result, Err(TspError::ParseError(_))));
    }

    #[test]
    fn test_parse_invalid_line_format() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : invalid_format
DIMENSION : 1
NODE_COORD_SECTION
1 0    # missing y coordinate
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(
            matches!(result, Err(TspError::ParseError(msg)) if msg.contains("Invalid coordinate line"))
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = Error::new(ErrorKind::NotFound, "file not found");
        let tsp_error = TspError::from(io_error);

        assert!(matches!(tsp_error, TspError::IoError(_)));
        if let TspError::IoError(err) = tsp_error {
            assert_eq!(err.kind(), ErrorKind::NotFound);
        } else {
            panic!("Expected IoError variant");
        }
    }

    #[test]
    fn test_parse_missing_node_coord_section() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : missing_section
DIMENSION : 1
1 0 0
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        // Since we never saw NODE_COORD_SECTION, no coordinates should be parsed
        assert!(
            matches!(result, Err(TspError::ParseError(msg)) if msg.contains("Expected 1 sites but found 0"))
        );
    }

    #[test]
    fn test_parse_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(matches!(result, Ok(sites) if sites.is_empty()));
    }

    #[test]
    fn test_parse_header_only() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : header_only
DIMENSION : 2
TYPE : TSP
EDGE_WEIGHT_TYPE : EUC_2D
NODE_COORD_SECTION
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        assert!(
            matches!(result, Err(TspError::ParseError(msg)) if msg.contains("Expected 2 sites but found 0"))
        );
    }

    #[test]
    fn test_parse_non_sequential_ids() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : non_sequential
DIMENSION : 2
NODE_COORD_SECTION
1 0 0
3 1 1
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        match result {
            Ok(sites) => {
                assert_eq!(sites.len(), 2, "Should have 2 sites");
                assert_eq!(sites[1].id, 3, "Second site should have id 3");
            }
            Err(e) => panic!("Expected Ok result, got error: {:?}", e),
        }
    }

    #[test]
    fn test_parse_duplicate_ids() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : duplicate_ids
DIMENSION : 2
NODE_COORD_SECTION
1 0 0
1 1 1
EOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        match result {
            Ok(sites) => {
                assert_eq!(sites.len(), 2, "Should have 2 sites");
                assert_eq!(sites[0].id, 1, "First site should have id 1");
                assert_eq!(sites[1].id, 1, "Second site should have id 1");
            }
            Err(e) => panic!("Expected Ok result, got error: {:?}", e),
        }
    }
}
