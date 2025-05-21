use log::debug;

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

/// Represents a TSP instance, which can be either a set of coordinates or an explicit distance matrix
#[derive(Debug, Clone)]
pub enum TspInstance {
    Coordinates(Vec<TspSite>),
    ExplicitMatrix(Vec<Vec<i32>>),
}

/// Parse a TSP file containing site coordinates or explicit edge weights.
///
/// Supports EDGE_WEIGHT_TYPE: EUC_2D (coordinates) and EXPLICIT (distance matrix, FULL_MATRIX only).
///
/// # Arguments
/// * `filename` - Path to the TSP file
///
/// # Returns
/// A TspInstance containing either coordinates or an explicit distance matrix
pub fn read_tsp_file(filename: &str) -> Result<TspInstance, TspError> {
    let mut sites = Vec::new();
    let mut in_coord_section = false;
    let mut in_weight_section = false;
    let mut dimension: Option<usize> = None;
    let mut edge_weight_type: Option<String> = None;
    let mut edge_weight_format: Option<String> = None;
    let mut matrix_data: Vec<i32> = Vec::new();

    for line in read_lines(filename)? {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line == "EOF" {
            continue;
        }

        if line == "NODE_COORD_SECTION" {
            in_coord_section = true;
            in_weight_section = false;
            continue;
        }
        if line == "EDGE_WEIGHT_SECTION" {
            in_weight_section = true;
            in_coord_section = false;
            continue;
        }

        if !in_coord_section && !in_weight_section {
            // Parse header section
            if line.starts_with("DIMENSION") {
                if let Some(dim_str) = line.split(':').nth(1) {
                    dimension = Some(dim_str.trim().parse().map_err(|_| {
                        TspError::ParseError("Invalid DIMENSION value".to_string())
                    })?);
                }
            } else if line.starts_with("EDGE_WEIGHT_TYPE") {
                if let Some(t) = line.split(':').nth(1) {
                    edge_weight_type = Some(t.trim().to_string());
                }
            } else if line.starts_with("EDGE_WEIGHT_FORMAT") {
                if let Some(f) = line.split(':').nth(1) {
                    edge_weight_format = Some(f.trim().to_string());
                }
            }
            continue;
        }

        if in_coord_section {
            // Parse coordinate section
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() != 3 {
                return Err(TspError::ParseError(format!(
                    "Invalid coordinate line: {line}"
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

            debug!("Parsed site: id={id}, x={x}, y={y}");

            sites.push(TspSite {
                id,
                coordinate: IPoint2D::new(x, y),
            });
        } else if in_weight_section {
            // Parse explicit edge weights (FULL_MATRIX only)
            let parts: Vec<&str> = line.split_whitespace().collect();
            for p in parts {
                let val = i32::from_str(p)
                    .map_err(|_| TspError::ParseError(format!("Invalid matrix value: {}", p)))?;
                matrix_data.push(val);
            }
        }
    }

    let edge_type = edge_weight_type.as_deref().unwrap_or("EUC_2D");
    match edge_type {
        "EUC_2D" => {
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
            Ok(TspInstance::Coordinates(sites))
        }
        "EXPLICIT" => {
            let dim = dimension.ok_or_else(|| {
                TspError::ParseError("Missing DIMENSION for EXPLICIT".to_string())
            })?;
            let format = edge_weight_format.as_deref().unwrap_or("FULL_MATRIX");
            match format {
                "FULL_MATRIX" => {
                    if matrix_data.len() != dim * dim {
                        return Err(TspError::ParseError(format!(
                            "Expected {} matrix entries but found {}",
                            dim * dim,
                            matrix_data.len()
                        )));
                    }
                    let mut matrix = Vec::with_capacity(dim);
                    for row in 0..dim {
                        let start = row * dim;
                        let end = start + dim;
                        matrix.push(matrix_data[start..end].to_vec());
                    }
                    Ok(TspInstance::ExplicitMatrix(matrix))
                }
                "LOWER_DIAG_ROW" => {
                    // The lower diagonal row format: each row i has i+1 entries (0-based)
                    let expected = (dim * (dim + 1)) / 2;
                    if matrix_data.len() != expected {
                        return Err(TspError::ParseError(format!(
                            "Expected {} matrix entries for LOWER_DIAG_ROW but found {}",
                            expected,
                            matrix_data.len()
                        )));
                    }
                    // Fill lower triangle and diagonal from input, upper triangle by symmetry
                    let mut matrix = vec![vec![0; dim]; dim];
                    let mut idx = 0;
                    for i in 0..dim {
                        for j in 0..=i {
                            let val = matrix_data[idx];
                            matrix[i][j] = val;
                            matrix[j][i] = val; // symmetric
                            idx += 1;
                        }
                    }
                    Ok(TspInstance::ExplicitMatrix(matrix))
                }
                other => Err(TspError::ParseError(format!(
                    "Only FULL_MATRIX and LOWER_DIAG_ROW formats are supported, got {}",
                    other
                ))),
            }
        }
        other => Err(TspError::ParseError(format!(
            "Unsupported EDGE_WEIGHT_TYPE: {}",
            other
        ))),
    }
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
        let instance = read_tsp_file(file.path().to_str().unwrap()).unwrap();

        match instance {
            TspInstance::Coordinates(sites) => {
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
            TspInstance::ExplicitMatrix(_) => panic!("Expected coordinates, got matrix"),
        }
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
        assert!(matches!(result, Ok(TspInstance::Coordinates(sites)) if sites.is_empty()));
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
            Ok(TspInstance::Coordinates(sites)) => {
                assert_eq!(sites.len(), 2, "Should have 2 sites");
                assert_eq!(sites[1].id, 3, "Second site should have id 3");
            }
            Ok(TspInstance::ExplicitMatrix(_)) => panic!("Expected coordinates, got matrix"),
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
            Ok(TspInstance::Coordinates(sites)) => {
                assert_eq!(sites.len(), 2, "Should have 2 sites");
                assert_eq!(sites[0].id, 1, "First site should have id 1");
                assert_eq!(sites[1].id, 1, "Second site should have id 1");
            }
            Ok(TspInstance::ExplicitMatrix(_)) => panic!("Expected coordinates, got matrix"),
            Err(e) => panic!("Expected Ok result, got error: {:?}", e),
        }
    }

    #[test]
    fn test_parse_explicit_full_matrix() {
        let mut file = NamedTempFile::new().unwrap();
        write!(
            file,
            "NAME : explicit_full_matrix\nDIMENSION : 3\nEDGE_WEIGHT_TYPE : EXPLICIT\nEDGE_WEIGHT_FORMAT : FULL_MATRIX\nEDGE_WEIGHT_SECTION\n0 1 2\n1 0 3\n2 3 0\nEOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        match result {
            Ok(TspInstance::ExplicitMatrix(matrix)) => {
                assert_eq!(matrix.len(), 3);
                assert_eq!(matrix[0], vec![0, 1, 2]);
                assert_eq!(matrix[1], vec![1, 0, 3]);
                assert_eq!(matrix[2], vec![2, 3, 0]);
            }
            Ok(TspInstance::Coordinates(_)) => panic!("Expected matrix, got coordinates"),
            Err(e) => panic!("Expected Ok result, got error: {:?}", e),
        }
    }

    #[test]
    fn test_parse_explicit_lower_diag_row() {
        let mut file = NamedTempFile::new().unwrap();
        // 3x3 matrix, lower diag row: 0, 1 0, 2 3 0
        // Should produce:
        // 0 1 2
        // 1 0 3
        // 2 3 0
        write!(
            file,
            "NAME : explicit_lower_diag_row\nDIMENSION : 3\nEDGE_WEIGHT_TYPE : EXPLICIT\nEDGE_WEIGHT_FORMAT : LOWER_DIAG_ROW\nEDGE_WEIGHT_SECTION\n0\n1 0\n2 3 0\nEOF"
        )
        .unwrap();

        let result = read_tsp_file(file.path().to_str().unwrap());
        match result {
            Ok(TspInstance::ExplicitMatrix(matrix)) => {
                assert_eq!(matrix.len(), 3);
                assert_eq!(matrix[0], vec![0, 1, 2]);
                assert_eq!(matrix[1], vec![1, 0, 3]);
                assert_eq!(matrix[2], vec![2, 3, 0]);
            }
            Ok(TspInstance::Coordinates(_)) => panic!("Expected matrix, got coordinates"),
            Err(e) => panic!("Expected Ok result, got error: {:?}", e),
        }
    }
}
