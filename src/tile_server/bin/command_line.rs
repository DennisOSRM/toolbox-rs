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

    /// path to the level directory that chipper wrote
    #[clap(short, long, action)]
    pub directory: String,

    /// The radius in metres of the disc that carves the alpha shape of a cell
    /// out of its convex hull. Larger gives the hull back, smaller eats into
    /// every bay of the cell until it falls into pieces.
    #[clap(short, long, default_value_t = 300.0, action)]
    pub alpha: f64,

    /// address and port to serve on
    #[clap(short, long, default_value = "127.0.0.1:5000", action)]
    pub listen: String,

    /// Time the tile builder over a sweep of zoom levels and partition levels
    /// and print what it costs, rather than serving anything.
    #[clap(long, action)]
    pub bench: bool,

    /// the zoom levels the sweep covers
    #[clap(long, value_delimiter = ',', default_value = "6,8,10,12,14", action)]
    pub bench_zooms: Vec<u32>,

    /// The side of the square of tiles timed at each point of the sweep. The
    /// first tile of a level pays for the hulls and the shapes of every cell
    /// on it, so it is reported apart from the rest.
    #[clap(long, default_value_t = 3, action)]
    pub bench_side: u32,

    /// where the sweep centres, as lat,lon
    #[clap(long, default_value = "50.20731,8.57747", action)]
    pub bench_at: String,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "coordinates: {}", self.coordinates)?;
        writeln!(f, "level directory: {}", self.directory)?;
        writeln!(f, "alpha: {} m", self.alpha)?;
        writeln!(f, "listen: {}", self.listen)
    }
}
