//! Checks that the cell distances of a partition say what the graph says.
//!
//! A cell is summarized by the distances between the nodes on its border, and
//! every level above the finest is worked out from the cells below rather than
//! from the graph. That is what makes a query fast and what makes it possible
//! to be quietly wrong: a cell of a million nodes is searched over a few
//! thousand, and nothing in that search ever looks at the graph again.
//!
//! So this tool goes the slow way round. Each cell is worked out the way a
//! query would have it, and then again from each of its border nodes by a
//! plain Dijkstra over the graph itself that knows nothing of levels. The two
//! have to agree on every ordered pair.
//!
//! ```text
//! sound -g graph.toolbox -d levels.bin -l 2
//! ```
//!
//! Without a level it checks all of them, which on a coarse level of a
//! continent is a long wait. It exits non-zero when a cell disagrees.

mod command_line;

use command_line::Arguments;
use env_logger::{Builder, Env};
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, warn};
use rayon::prelude::*;
use std::{error::Error, time::Instant};
use toolbox_rs::{
    customization::{CellCheck, Customization, Mismatch},
    graph::Graph,
    io,
    level_directory::{CellId, LevelDirectory},
    static_graph,
};

/// How many cells are checked before what was worked out below them is
/// dropped. A cell of a level belongs to exactly one cell of the level above,
/// so nothing is thrown away that a later batch would have asked for again,
/// and the tables of a whole level of a continent never have to be held at
/// once.
const BATCH: usize = 256;

/// What checking a whole level came to.
struct LevelCheck {
    pairs: u64,
    /// cells that hold no border node, which cannot be entered or left and so
    /// have nothing to check
    without_border: u64,
    /// how many distances differ from the graph in all
    wrong: u64,
    /// the first of them, as many as were asked to be reported. A directory
    /// that is broken through and through has as many mismatches as it has
    /// pairs, and holding all of them to print twenty is a way to run out of
    /// memory while reporting a fault.
    mismatches: Vec<Mismatch>,
}

fn check_level(customization: &mut Customization, level: usize, keep: usize) -> LevelCheck {
    let cells = customization.level(level);
    let count = cells.cells();
    info!(
        "checking {count} cells of level {level}, the largest holding {} nodes",
        (0..cells.cells())
            .map(|cell| cells.nodes_of(cell as u32).len())
            .max()
            .unwrap_or(0)
    );

    let bar = ProgressBar::new(count as u64);
    bar.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} [{elapsed_precise}] {wide_bar:.green/yellow} {msg}")
            .unwrap()
            .progress_chars("#>-"),
    );

    let mut found = LevelCheck {
        pairs: 0,
        without_border: 0,
        wrong: 0,
        mismatches: Vec::new(),
    };
    for start in (0..count as CellId).step_by(BATCH) {
        let end = (start + BATCH as CellId).min(count as CellId);
        // shared for the searching, and asked for again below to let the
        // tables of the batch go
        let shared = &*customization;
        let checks = (start..end)
            .into_par_iter()
            .map(|cell| shared.check(level, cell))
            .collect::<Vec<CellCheck>>();

        for check in checks {
            found.pairs += check.pairs;
            found.without_border += u64::from(!check.has_border);
            found.wrong += check.mismatches.len() as u64;
            let room = keep.saturating_sub(found.mismatches.len());
            found
                .mismatches
                .extend(check.mismatches.into_iter().take(room));
        }
        customization.forget();
        bar.inc(u64::from(end - start));
        bar.set_message(format!("{} pairs, {} wrong", found.pairs, found.wrong));
    }
    bar.finish();
    found
}

fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#"                             _  "#);
    println!(r#"  ___    ___    _  _   _ _  | | "#);
    println!(r#" (_-<   / _ \  | +| | | ' \ |_| "#);
    println!(r#" /__/_  \___/   \_,_| |_||_|(_) "#);
    println!(r#"_|"""""|"""""|_|"""""|_|"""""|  "#);
    println!(r#""`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-' "#);
    println!("build: {}", env!("GIT_HASH"));
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let edges = io::read_edges_from_file(&args.graph);
    info!("loaded {} graph edges", edges.len());
    let directory: LevelDirectory = io::read_from_file(&args.directory);
    info!(
        "loaded a directory of {} levels over {} nodes",
        directory.levels(),
        directory.number_of_nodes()
    );

    let graph = static_graph::StaticGraph::new(edges);
    info!(
        "loaded static graph with {} nodes and {} edges",
        graph.number_of_nodes(),
        graph.number_of_edges()
    );
    let levels = match args.level {
        Some(level) => {
            assert!(
                level < directory.levels(),
                "the directory has no level {level}"
            );
            level..level + 1
        }
        None => 0..directory.levels(),
    };
    let mut customization = Customization::new(graph, directory);

    let started = Instant::now();
    let mut sound = true;
    for level in levels {
        let found = check_level(&mut customization, level, args.report);
        info!(
            "level {level}: checked {} pairs over {} cells, leaving out {} that hold no border node",
            found.pairs,
            customization.level(level).cells() as u64 - found.without_border,
            found.without_border
        );
        if found.wrong == 0 {
            info!("level {level} says what the graph says");
            continue;
        }

        sound = false;
        warn!("level {level}: {} pairs differ from the graph", found.wrong);
        for wrong in &found.mismatches {
            let expected = if wrong.expected == usize::MAX {
                "unreachable".to_owned()
            } else {
                wrong.expected.to_string()
            };
            warn!(
                "  cell {}, node {} to node {}: built {}, graph {expected}",
                wrong.cell, wrong.from, wrong.to, wrong.built
            );
        }
        if found.wrong > found.mismatches.len() as u64 {
            warn!("  and {} more", found.wrong - found.mismatches.len() as u64);
        }
    }

    info!(
        "customized {} cells in {:.1?}, checked in {:.1?}",
        customization.customized_cells(),
        customization.customization_time(),
        started.elapsed()
    );
    if sound {
        Ok(())
    } else {
        Err("the cell distances do not match the graph".into())
    }
}
