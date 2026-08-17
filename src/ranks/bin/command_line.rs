use std::fmt::Display;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Arguments {
    #[command(subcommand)]
    pub mode: Mode,
}

#[derive(Subcommand, Debug)]
pub enum Mode {
    /// Draws pairs of nodes and says where each of them sits on the rank axis.
    ///
    /// A source is drawn at random and searched from over the whole graph, and
    /// the node settled at each power of two is written down with the rank it
    /// was settled at. One search hands back a pair for every rank, which is
    /// what makes a sample of a million affordable: a search per pair would
    /// cost the same as searching a source for every one of them.
    Sample(Sample),

    /// Holds the search over the cells against a search through them.
    ///
    /// Every pair of the file is asked of both, and a distance they disagree
    /// on is a fault in one of them. The pairs of a source are asked of the
    /// query together rather than one at a time, as a query with a set of
    /// targets has to leave every cell holding one of them alone and that is
    /// not exercised by asking for a single target.
    Check(Check),

    /// Times the pairs, and counts nothing while doing it.
    ///
    /// A run that is being counted is not a run worth timing, so this reads
    /// the pairs back and searches them again with nothing collecting.
    Time(Time),
}

#[derive(Parser, Debug)]
pub struct Sample {
    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// how many sources to draw
    #[clap(short, long, default_value_t = 1000, action)]
    pub sources: usize,

    /// what to seed the drawing with, so a sample can be drawn again
    #[clap(long, default_value_t = 0x_5EED, action)]
    pub seed: u64,

    /// where to write the pairs
    #[clap(short, long, action)]
    pub out: String,

    /// how many threads to search on, and all of them when left out
    #[clap(short, long, action)]
    pub threads: Option<usize>,

    /// Draw pairs of nodes at random instead, and search each pair on its own.
    ///
    /// This is what was asked for rather than what is affordable: a target
    /// drawn at random sits at a high rank almost every time, so the low ranks
    /// come out empty, and every pair costs a search of its own.
    #[clap(long, action)]
    pub pairs: bool,
}

#[derive(Parser, Debug)]
pub struct Check {
    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the level directory that chipper wrote
    #[clap(short, long, action)]
    pub directory: String,

    /// the pairs to check, as written by the sample mode
    #[clap(short, long, action)]
    pub input: String,

    /// how many disagreements to print before saying only how many there were
    #[clap(long, default_value_t = 10, action)]
    pub report: usize,
}

#[derive(Parser, Debug)]
pub struct Time {
    /// path to the input graph
    #[clap(short, long, action)]
    pub graph: String,

    /// path to the level directory that chipper wrote, for the mld engine
    #[clap(short, long, default_value_t = String::new(), action)]
    pub directory: String,

    /// which search to time
    #[clap(short, long, value_enum, default_value_t = Engine::Dijkstra)]
    pub engine: Engine,

    /// the pairs to time, as written by the sample mode
    #[clap(short, long, action)]
    pub input: String,

    /// where to write the timings
    #[clap(short, long, action)]
    pub out: String,

    /// How many pairs to run before the clock is started, so that the first of
    /// them does not pay for what the rest of them find already warm.
    #[clap(long, default_value_t = 100, action)]
    pub warmup: usize,

    /// What to seed the shuffling with, so a run can be repeated.
    #[clap(long, default_value_t = 0x_5EED, action)]
    pub seed: u64,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// a plain unidirectional Dijkstra over the graph
    Dijkstra,
    /// a search over the cells of the partition
    Mld,
}

impl Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Engine::Dijkstra => write!(f, "dijkstra"),
            Engine::Mld => write!(f, "mld"),
        }
    }
}

impl Display for Arguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "command line arguments:")?;
        match &self.mode {
            Mode::Sample(sample) => {
                writeln!(f, "mode: sample")?;
                writeln!(f, "graph: {}", sample.graph)?;
                writeln!(f, "sources: {}", sample.sources)?;
                writeln!(f, "seed: {}", sample.seed)?;
                writeln!(f, "out: {}", sample.out)?;
                writeln!(f, "pairs of nodes drawn at random: {}", sample.pairs)
            }
            Mode::Check(check) => {
                writeln!(f, "mode: check")?;
                writeln!(f, "graph: {}", check.graph)?;
                writeln!(f, "level directory: {}", check.directory)?;
                writeln!(f, "in: {}", check.input)
            }
            Mode::Time(time) => {
                writeln!(f, "mode: time")?;
                writeln!(f, "graph: {}", time.graph)?;
                writeln!(f, "level directory: {}", time.directory)?;
                writeln!(f, "engine: {}", time.engine)?;
                writeln!(f, "in: {}", time.input)?;
                writeln!(f, "out: {}", time.out)?;
                writeln!(f, "warmup: {} pairs", time.warmup)?;
                writeln!(f, "seed: {}", time.seed)
            }
        }
    }
}
