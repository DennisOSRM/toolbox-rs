use std::fmt::Display;

use clap::{Parser, ValueEnum};

#[derive(ValueEnum, Clone, Debug)]
pub enum InputFormat {
    Dimacs,
    Ddsg,
    Metis,
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// input graph file
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the input coordinates
    #[clap(short, long, action)]
    pub coordinates: String,

    /// path to the assignment file
    #[clap(short, long, action)]
    pub assignment: String,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "coordinates: {}", self.coordinates)?;
        writeln!(f, "assignment: {}", self.assignment)
    }
}
