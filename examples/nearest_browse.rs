//! What the nearest node and the nearest piece of road cost, held and paged.
//!
//!   nearest_browse <graph> <coordinates> <index> [MiB ...]
//!
//! Builds an index over the nodes and one over the arcs, writes both, and asks
//! each of them the same questions twice over: once with the whole thing in
//! memory and once reading it off the file through a pool of the given size.
//! The places asked about are drawn at random from the box the data lies in,
//! which is the case a map server sees -- a coordinate off a phone is not a
//! node of the graph and is not near one in any particular way.
//!
//! TOOLBOX_ASKED     how many places to ask about
//! TOOLBOX_TIMINGS   per-question rows, for a plot

use std::{env::args, fs::File, io::Write, path::Path, time::Instant};

use rand::{RngExt, SeedableRng, rngs::StdRng};
use toolbox_rs::{
    bounding_box::BoundingBox, geometry::FPCoordinate, graph::Graph, io, nearest::NearestIndex,
    pool::Pool, static_graph::StaticGraph,
};

const MIB: usize = 1024 * 1024;

fn main() {
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: nearest_browse <graph> <coordinates> <index> [MiB ...]: missing {what}")
        })
    };
    let graph_path = next("graph");
    let coordinates_path = next("coordinates");
    let index_path = next("index");
    let budgets: Vec<usize> = argv
        .map(|mib| mib.parse::<usize>().expect("a size in MiB") * MIB)
        .collect();
    let budgets = if budgets.is_empty() {
        vec![MIB, 8 * MIB, 64 * MIB]
    } else {
        budgets
    };

    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates_path);
    println!(
        "{} nodes, {} arcs, {} coordinates",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        coordinates.len()
    );

    let started = Instant::now();
    let over_nodes = NearestIndex::over_nodes(&coordinates);
    let by_nodes = started.elapsed();
    let started = Instant::now();
    let over_segments = NearestIndex::over_segments(&graph, &coordinates);
    let by_segments = started.elapsed();
    println!(
        "built {} node boxes over {} nodes in {by_nodes:.1?}, and {} over {} segments in {by_segments:.1?}",
        over_nodes.levels(),
        over_nodes.len(),
        over_segments.levels(),
        over_segments.len(),
    );

    let nodes_at = format!("{index_path}.nodes");
    let segments_at = format!("{index_path}.segments");
    over_nodes
        .save(Path::new(&nodes_at))
        .expect("a node index to write");
    over_segments
        .save(Path::new(&segments_at))
        .expect("a segment index to write");
    for (what, at) in [("nodes", &nodes_at), ("segments", &segments_at)] {
        let held = std::fs::metadata(at).expect("a file").len();
        println!(
            "the {what} index is {:.1} MiB on the file",
            held as f64 / MIB as f64
        );
    }

    // Places drawn from the box the data lies in, which is what a map server
    // is asked about: a coordinate off a phone is not a node and is not near
    // one in any particular way. The same seed every run, so two runs ask the
    // same questions.
    let asked: usize = std::env::var("TOOLBOX_ASKED")
        .ok()
        .and_then(|how| how.parse().ok())
        .unwrap_or(10_000);
    let whole = BoundingBox::from_coordinates(&coordinates);
    let (min, max) = whole.corners();
    let mut rng = StdRng::seed_from_u64(0x5EED);
    let places: Vec<FPCoordinate> = (0..asked)
        .map(|_| {
            FPCoordinate::new(
                rng.random_range(min.lat..=max.lat),
                rng.random_range(min.lon..=max.lon),
            )
        })
        .collect();
    println!("{asked} places drawn from the box the data lies in");

    let writing = std::env::var("TOOLBOX_TIMINGS").ok();
    let mut rows = String::new();

    println!(
        "\n{:>10} {:>10} {:>10} {:>10} {:>10} {:>9}",
        "over", "budget", "median", "p95", "in memory", "slowdown"
    );
    for (what, whole, at) in [
        ("nodes", &over_nodes, &nodes_at),
        ("segments", &over_segments, &segments_at),
    ] {
        // in memory first, which is what the paged one is measured against
        let mut held_took = Vec::with_capacity(asked);
        let mut answers = Vec::with_capacity(asked);
        for &place in &places {
            let started = Instant::now();
            let found = whole.nearest(place);
            held_took.push(started.elapsed().as_nanos() as u64);
            answers.push(found.expect("something is nearest"));
        }
        held_took.sort_unstable();
        let in_memory = held_took[held_took.len() / 2] as f64 / 1000.0;
        println!(
            "{what:>10} {:>10} {:>10} {:>10} {:>10} {:>9}",
            "held",
            format!("{in_memory:.1}us"),
            format!(
                "{:.1}us",
                held_took[held_took.len() * 95 / 100] as f64 / 1000.0
            ),
            "-",
            "1.00x",
        );
        if writing.is_some() {
            for (place, took) in places.iter().zip(&held_took) {
                use std::fmt::Write as _;
                let _ = writeln!(rows, "{what},held,0,{},{},{took}", place.lat, place.lon);
            }
        }

        for &bytes in &budgets {
            let pool = Pool::of(bytes);
            let read = NearestIndex::open(Path::new(at), &pool).expect("an index to read");
            let mut took = Vec::with_capacity(asked);
            let mut wrong = 0_usize;
            for (which, &place) in places.iter().enumerate() {
                let started = Instant::now();
                let found = read.nearest(place);
                took.push(started.elapsed().as_nanos() as u64);
                if found != Some(answers[which]) {
                    wrong += 1;
                }
            }
            assert_eq!(
                wrong, 0,
                "the paged index answered differently {wrong} times"
            );
            took.sort_unstable();
            let median = took[took.len() / 2] as f64 / 1000.0;
            println!(
                "{what:>10} {:>10} {:>10} {:>10} {:>10} {:>9}",
                format!("{} MiB", bytes / MIB),
                format!("{median:.1}us"),
                format!("{:.1}us", took[took.len() * 95 / 100] as f64 / 1000.0),
                format!("{in_memory:.1}us"),
                format!("{:.2}x", median / in_memory),
            );
            if writing.is_some() {
                use std::fmt::Write as _;
                for (place, one) in places.iter().zip(&took) {
                    let _ = writeln!(
                        rows,
                        "{what},paged,{},{},{},{one}",
                        bytes / MIB,
                        place.lat,
                        place.lon
                    );
                }
            }
            println!(
                "{:>10} {:>10} {} blocks read, {:.1} MiB held of {:.1}",
                "",
                "",
                pool.faults().reads,
                pool.faults().held as f64 / MIB as f64,
                bytes as f64 / MIB as f64,
            );
        }
    }

    if let Some(at) = &writing {
        let mut out = File::create(at).expect("somewhere to write the timings");
        writeln!(out, "over,how,budget,lat,lon,nanos").expect("a header");
        out.write_all(rows.as_bytes()).expect("the timings");
        println!("\nwrote {at}");
    }
}
