use std::fmt::Display;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// input graph file
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the level directory that chipper wrote
    #[clap(short, long, action)]
    pub directory: String,

    /// The level to check, and every level of the directory when none is
    /// given. A level is checked against the graph itself, so a coarse one
    /// costs a search over each of its cells per border node it holds.
    #[clap(short, long, action)]
    pub level: Option<usize>,

    /// how many mismatches to report before the rest is counted only
    #[clap(short, long, default_value_t = 20, action)]
    pub report: usize,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "level directory: {}", self.directory)?;
        match self.level {
            Some(level) => writeln!(f, "level: {level}")?,
            None => writeln!(f, "level: all of them")?,
        }
        writeln!(f, "report: {} mismatches", self.report)
    }
}
