use std::fmt::Display;

use clap::Parser;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// cell assignments from chipper tool
    #[clap(short, long, action)]
    pub partition_file: String,
    /// input coordinates files
    #[clap(short, long, action)]
    pub coordinates_file: String,
    /// input graph file
    #[clap(short, long, action)]
    pub graph: String,

    /// output convex hull cell geometry
    #[clap(long, action, default_value_t = String::new())]
    pub convex_cells_geojson: String,
    /// output boundary node locations
    #[clap(long, action, default_value_t = String::new())]
    pub boundary_nodes_geojson: String,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "partition_file: {}", self.partition_file)?;
        writeln!(f, "coordinates_file: {}", self.coordinates_file)?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "convex_cells_geojson: {}", self.convex_cells_geojson)?;
        writeln!(f, "boundary_nodes_geojson: {}", self.boundary_nodes_geojson)
    }
}
