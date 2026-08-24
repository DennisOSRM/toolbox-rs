use std::fmt::Display;

use clap::Parser;

/// Numbers the nodes of an instance so that the ones a search over the cells
/// reads come first, and writes the instance back out under those numbers.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the level directory that chipper wrote
    #[clap(short, long, action)]
    pub directory: String,

    /// path to the coordinates, which are held at the node they belong to and
    /// so have to move with them. Left out when there are none to move.
    #[clap(short, long, default_value_t = String::new(), action)]
    pub coordinates: String,

    /// where to write the renumbered graph
    #[clap(long, action)]
    pub out_graph: String,

    /// where to write the renumbered directory
    #[clap(long, action)]
    pub out_directory: String,

    /// where to write the renumbered coordinates
    #[clap(long, default_value_t = String::new(), action)]
    pub out_coordinates: String,

    /// Where to write the numbering itself.
    ///
    /// Whoever asks a question of the renumbered instance asks it about a node
    /// of the input, so the numbering has to be kept or the answers cannot be
    /// read back.
    #[clap(long, action)]
    pub out_ordering: String,

    /// Which order to number in.
    ///
    /// `border-first` puts the border nodes of the coarsest cells at the front
    /// of the whole graph, which is what a search in memory wants.
    /// `cell-path` keeps every cell, and so every subtree, in one run of
    /// numbers, which is what a store that hands out a subtree at a time
    /// wants. Measured over europe.ptv the second costs a fifth of the median
    /// query, all of it on the long routes.
    #[clap(long, value_enum, default_value_t = Order::BorderFirst)]
    pub numbering: Order,

    /// Number the cells of each level in the order their keys run, before
    /// numbering the nodes.
    ///
    /// Out of the assembly a cell is numbered as the merging happened to reach
    /// it, so a run of cell numbers is not a range of keys. Renumbered, a
    /// block of a store is a range of keys, of cell numbers and of node
    /// numbers at once. Says nothing about which cell a node is in.
    #[clap(long, action)]
    pub cells_in_key_order: bool,
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        writeln!(f, "graph: {}", self.graph)?;
        writeln!(f, "level directory: {}", self.directory)?;
        writeln!(f, "coordinates: {}", self.coordinates)?;
        writeln!(f, "out graph: {}", self.out_graph)?;
        writeln!(f, "out directory: {}", self.out_directory)?;
        writeln!(f, "out coordinates: {}", self.out_coordinates)?;
        writeln!(f, "out ordering: {}", self.out_ordering)
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    BorderFirst,
    CellPath,
}

impl From<Order> for toolbox_rs::node_ordering::Numbering {
    fn from(order: Order) -> Self {
        match order {
            Order::BorderFirst => Self::BorderFirst,
            Order::CellPath => Self::CellPath,
        }
    }
}
