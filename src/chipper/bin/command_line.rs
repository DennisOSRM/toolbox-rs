use std::{fmt::Display, ops::RangeInclusive};

use clap::Parser;

static RECURSION_RANGE: RangeInclusive<u8> = 1..=31;
static BALANCE_RANGE: RangeInclusive<f64> = 0. ..=0.5;

/// Checks whether the recursion range is within the expected range of (1, 31].
pub fn recursion_depth_in_range(s: &str) -> Result<u8, String> {
    let recursion_depth: u8 = s.parse().map_err(|_| format!("`{s}` isn't a number"))?;
    if RECURSION_RANGE.contains(&recursion_depth) {
        Ok(recursion_depth)
    } else {
        Err(format!(
            "recursion range not in range {}-{}",
            RECURSION_RANGE.start(),
            RECURSION_RANGE.end()
        ))
    }
}

/// Checks whether the balance factor is within the expected range of (0.,0.5]
pub fn balance_factor_in_range(s: &str) -> Result<f64, String> {
    let factor: f64 = s.parse().map_err(|_| format!("`{s}` isn't a number"))?;
    if BALANCE_RANGE.contains(&factor) {
        Ok(factor)
    } else {
        Err(format!(
            "balance factor not in range {}-{}",
            BALANCE_RANGE.start(),
            BALANCE_RANGE.end()
        ))
    }
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// Number of threads to use
    #[clap(short, long, action)]
    pub number_of_threads: Option<usize>,

    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the input coordinates
    #[clap(short, long, action)]
    pub coordinates: String,

    /// path to the cut-csv file
    #[clap(short = 'o', long, default_value_t = String::new(), action)]
    pub cut_csv: String,

    /// path to the assignment-csv file
    #[clap(short, long, default_value_t = String::new(), action)]
    pub assignment_csv: String,

    /// balance factor to use
    #[clap(short, long, value_parser = balance_factor_in_range, default_value_t = 0.25)]
    pub b_factor: f64,

    /// depth of recursive partitioning; off by one from the level of a node
    /// since the root node has level 1, e.g. depths of 1 gives cells on level 2
    #[clap(short, long, value_parser=recursion_depth_in_range, default_value_t = 1)]
    pub recursion_depth: u8,

    /// path to the output file with partition ids
    #[clap(short, long, default_value_t = String::new(), action)]
    pub partition_file: String,

    /// Minimum size of a cell
    #[clap(short, long, default_value_t = 50, action)]
    pub minimum_cell_size: usize,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        if let Some(number_of_threads) = self.number_of_threads {
            writeln!(f, "number_of_threads: {number_of_threads}")?;
        }
        if !self.partition_file.is_empty() {
            writeln!(f, "output partition file: {}", self.partition_file)?;
        }
        if !self.assignment_csv.is_empty() {
            writeln!(f, "assignment csv: {}", self.assignment_csv)?;
        }
        if !self.cut_csv.is_empty() {
            writeln!(f, "cut csv: {}", self.cut_csv)?;
        }
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "coordinates: {}", self.coordinates)?;
        writeln!(f, "recursion depth: {}", self.recursion_depth)?;
        writeln!(f, "balance factor: {}", self.b_factor)?;
        writeln!(f, "minimum_cell_size: {}", self.minimum_cell_size)
    }
}
