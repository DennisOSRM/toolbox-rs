use std::fmt::Display;

use clap::ArgEnum;
use clap::Parser;

#[derive(ArgEnum, Clone, Debug)]
pub enum InputFormat {
    DIMACS,
    DDSG,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// Number of threads to use
    #[clap(short, long, action)]
    pub input_format: InputFormat,

    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the input coordinates
    #[clap(short, long, action)]
    pub coordinates: String,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "coordinates: {}", self.coordinates)
    }
}
