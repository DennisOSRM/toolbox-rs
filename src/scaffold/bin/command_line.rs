use std::fmt::Display;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// input files
    #[clap(short, long, action)]
    pub partition_file: String,
    #[clap(short, long, action)]
    pub coordinates_file: String,

    /// output files
    #[clap(long, action)]
    pub convex_cells_geojson: String,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "partition_file: {}", self.partition_file)?;
        writeln!(f, "coordinates_file: {}", self.coordinates_file)
    }
}
