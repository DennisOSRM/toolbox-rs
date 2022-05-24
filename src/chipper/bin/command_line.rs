use std::{fmt::Display, ops::RangeInclusive};

use clap::Parser;

static LEVEL_RANGE: RangeInclusive<u8> = 1..=31;
static BALANCE_RANGE: RangeInclusive<f64> = 0. ..=0.5;

/// Checks whether the target level is within the expected range of (1, 31].
pub fn target_level_in_range(s: &str) -> Result<u8, String> {
    let target_level: u8 = s.parse().map_err(|_| format!("`{}` isn't a number", s))?;
    if LEVEL_RANGE.contains(&target_level) {
        Ok(target_level)
    } else {
        Err(format!(
            "target level not in range {}-{}",
            LEVEL_RANGE.start(),
            LEVEL_RANGE.end()
        ))
    }
}

/// Checks whether the balance factor is within the expected range of (0.,0.5]
pub fn balance_factor_in_range(s: &str) -> Result<f64, String> {
    let factor: f64 = s.parse().map_err(|_| format!("`{}` isn't a number", s))?;
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
    /// path to the input graph
    #[clap(short, long)]
    pub graph: String,

    /// path to the input coordinates
    #[clap(short, long)]
    pub coordinates: String,

    /// path to the cut-csv file
    #[clap(short = 'o', long, default_value_t = String::new())]
    pub cut_csv: String,

    /// path to the assignment-csv file
    #[clap(short, long, default_value_t = String::new())]
    pub assignment_csv: String,

    /// balance factor to use
    #[clap(short, long, parse(try_from_str=balance_factor_in_range), default_value_t = 0.25)]
    pub b_factor: f64,

    /// target level of the resulting partition
    #[clap(short, long, parse(try_from_str=target_level_in_range), default_value_t = 1)]
    pub target_level: u8,

    /// path to the output file with partition ids
    #[clap(short, long, default_value_t = String::new())]
    pub partition_file: String,

    /// Minimum size of a cell
    #[clap(short, long, default_value_t = 50)]
    pub minimum_cell_size: usize,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "coordinates: {}", self.coordinates)?;
        writeln!(f, "target level: {}", self.target_level)?;
        writeln!(f, "balance factor: {}", self.b_factor)
    }
}
