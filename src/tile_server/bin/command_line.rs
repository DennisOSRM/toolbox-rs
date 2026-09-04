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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The three paths are required, so every parse has to carry them.
    fn parsed(extra: &[&str]) -> Arguments {
        let mut args = vec![
            "tile_server",
            "--graph",
            "graph.dimacs",
            "--coordinates",
            "coordinates.bin",
            "--directory",
            "levels.bin",
        ];
        args.extend_from_slice(extra);
        Arguments::parse_from(args)
    }

    #[test]
    fn the_command_line_is_well_formed() {
        // clap's own check: no two options sharing a short flag, no default
        // that its own parser would reject, and so on
        Arguments::command().debug_assert();
    }

    #[test]
    fn the_paths_are_read_off_the_command_line() {
        let arguments = parsed(&[]);
        assert_eq!(arguments.graph, "graph.dimacs");
        assert_eq!(arguments.coordinates, "coordinates.bin");
        assert_eq!(arguments.directory, "levels.bin");
    }

    #[test]
    fn what_is_not_given_falls_back_to_a_default() {
        let arguments = parsed(&[]);
        assert_eq!(arguments.alpha, 300.0);
        assert_eq!(arguments.listen, "127.0.0.1:5000");
        assert!(!arguments.bench);
        assert_eq!(arguments.bench_zooms, vec![6, 8, 10, 12, 14]);
        assert_eq!(arguments.bench_side, 3);
        assert_eq!(arguments.bench_at, "50.20731,8.57747");
    }

    #[test]
    fn the_zooms_of_the_sweep_are_a_comma_separated_list() {
        assert_eq!(
            parsed(&["--bench-zooms", "7,9,11"]).bench_zooms,
            vec![7, 9, 11]
        );
    }

    #[test]
    fn one_zoom_is_a_list_of_one() {
        assert_eq!(parsed(&["--bench-zooms", "12"]).bench_zooms, vec![12]);
    }

    #[test]
    fn the_sweep_is_asked_for_by_a_flag_that_carries_no_value() {
        assert!(parsed(&["--bench"]).bench);
    }

    #[test]
    fn a_short_flag_says_the_same_as_its_long_one() {
        let long = parsed(&["--alpha", "50", "--listen", "0.0.0.0:80"]);
        let short = parsed(&["-a", "50", "-l", "0.0.0.0:80"]);
        assert_eq!(long.alpha, short.alpha);
        assert_eq!(long.listen, short.listen);
    }

    #[test]
    fn a_missing_path_is_refused_rather_than_defaulted() {
        // no --directory, which has no default and cannot be guessed
        assert!(
            Arguments::try_parse_from([
                "tile_server",
                "--graph",
                "graph.dimacs",
                "--coordinates",
                "coordinates.bin",
            ])
            .is_err()
        );
    }

    #[test]
    fn what_is_displayed_names_every_path_it_was_given() {
        let shown = parsed(&[]).to_string();
        for expected in [
            "graph.dimacs",
            "coordinates.bin",
            "levels.bin",
            "300",
            "127.0.0.1:5000",
        ] {
            assert!(
                shown.contains(expected),
                "{shown:?} does not say {expected}"
            );
        }
    }
}
