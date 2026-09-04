//! What software prefetching is worth to a breadth-first search.
//!
//!     bfs_prefetch                 grids only
//!     bfs_prefetch <graph>         grids and a road network
use std::{collections::VecDeque, env::args, time::Instant};

use toolbox_rs::{
    edge::InputEdge,
    io,
    prefetch::{hint, last_level_cache, worth_it},
};

/// How many bytes a search over this graph touches at random: the offsets, the
/// arcs, and a settled flag a node.
fn working_set(nodes: usize, arcs: usize) -> usize {
    4 * (nodes + 1 + arcs + nodes)
}

/// A graph as an adjacency array: where each node's arcs begin, and where they go.
struct Csr {
    begins: Vec<u32>,
    targets: Vec<u32>,
}

impl Csr {
    fn of(edges: &[(u32, u32)], nodes: usize) -> Self {
        let mut begins = vec![0_u32; nodes + 1];
        for &(from, _) in edges {
            begins[from as usize + 1] += 1;
        }
        for i in 1..begins.len() {
            begins[i] += begins[i - 1];
        }
        let mut cursor = begins.clone();
        let mut targets = vec![0_u32; edges.len()];
        for &(from, to) in edges {
            targets[cursor[from as usize] as usize] = to;
            cursor[from as usize] += 1;
        }
        Self { begins, targets }
    }

    fn nodes(&self) -> usize {
        self.begins.len() - 1
    }
}

/// The plain search.
fn bfs(csr: &Csr, source: u32, seen: &mut [u32], queue: &mut VecDeque<u32>) -> usize {
    seen.fill(u32::MAX);
    queue.clear();
    queue.push_back(source);
    seen[source as usize] = source;

    let mut settled = 0;
    while let Some(node) = queue.pop_front() {
        settled += 1;
        let node = node as usize;
        let (from, to) = (csr.begins[node] as usize, csr.begins[node + 1] as usize);
        for edge in from..to {
            let target = csr.targets[edge] as usize;
            if seen[target] == u32::MAX {
                seen[target] = node as u32;
                queue.push_back(target as u32);
            }
        }
    }
    settled
}

/// How far ahead a hint is issued, in queue places for the first two and in
/// arcs for the third.
///
/// Swept over 24 pairs on a grid of four million nodes, where the whole surface
/// lies between 65 and 69 per cent saved: the distances are not what the
/// technique turns on. What holds everywhere is the gap between the two, since
/// the offsets hint must land before the arc hint reads them.
///
/// Which arc distance wins is a property of the instance, over four rounds of
/// four each way: a grid wants two, at 69.1 per cent against 68.0 for six,
/// while the road network wants six, at 26.5 per cent against 24.2 for two. A
/// grid's four arcs a node are one line, so the block hint is worth little and
/// is best issued close to its use; a road network's blocks are scattered
/// enough to want the longer run-up. Two is the default here because the grids
/// come first; `PushRelabel` uses six, since a road network is its workload.
/// Overridable so the sweep can be repeated elsewhere.
fn ahead() -> (usize, usize) {
    let of = |name: &str, fallback: usize| {
        std::env::var(name)
            .ok()
            .and_then(|given| given.parse().ok())
            .unwrap_or(fallback)
    };
    (of("TOOLBOX_OFFSETS_AHEAD", 12), of("TOOLBOX_ARCS_AHEAD", 2))
}

/// The same search, asking for what it is about to want.
fn bfs_prefetching(csr: &Csr, source: u32, seen: &mut [u32], queue: &mut VecDeque<u32>) -> usize {
    let (offsets_ahead, arcs_ahead) = ahead();
    // which of the three sites are in play, for taking them away one at a time
    let site = |name: &str| std::env::var(name).is_err();
    let (do_offsets, do_arcs) = (site("TOOLBOX_NO_OFFSETS"), site("TOOLBOX_NO_ARCS"));
    seen.fill(u32::MAX);
    queue.clear();
    queue.push_back(source);
    seen[source as usize] = source;

    let mut settled = 0;
    while let Some(node) = queue.pop_front() {
        settled += 1;
        // Two stages, because the second address is not known until the first
        // has landed: the offsets of a node further down the queue, and the arcs
        // of a nearer one, whose offsets were asked for several rounds ago and
        // are in cache by now.
        if do_offsets && let Some(&soon) = queue.get(offsets_ahead) {
            hint(std::ptr::from_ref(&csr.begins[soon as usize]));
        }
        if do_arcs && let Some(&soon) = queue.get(arcs_ahead) {
            hint(std::ptr::from_ref(
                &csr.targets[csr.begins[soon as usize] as usize],
            ));
        }

        let node = node as usize;
        let (from, to) = (csr.begins[node] as usize, csr.begins[node + 1] as usize);
        for edge in from..to {
            let target = csr.targets[edge] as usize;
            if seen[target] == u32::MAX {
                seen[target] = node as u32;
                queue.push_back(target as u32);
            }
        }
    }
    settled
}

/// A four-connected grid, both ways along every arc.
fn grid(side: usize) -> (Vec<(u32, u32)>, usize) {
    let node = |row: usize, column: usize| (row * side + column) as u32;
    let mut edges = Vec::with_capacity(4 * side * side);
    for row in 0..side {
        for column in 0..side {
            if column + 1 < side {
                edges.push((node(row, column), node(row, column + 1)));
                edges.push((node(row, column + 1), node(row, column)));
            }
            if row + 1 < side {
                edges.push((node(row, column), node(row + 1, column)));
                edges.push((node(row + 1, column), node(row, column)));
            }
        }
    }
    (edges, side * side)
}

/// Whether a search over this graph is worth prefetching for.
///
/// One times the last level cache is where the measurements put the line: a
/// grid of a million nodes, at 2.8 times the cache, still saves about half.
fn worth_prefetching(nodes: usize, arcs: usize) -> bool {
    worth_it(working_set(nodes, arcs))
}

fn time(what: &str, csr: &Csr, rounds: usize) {
    let mut seen = vec![u32::MAX; csr.nodes()];
    let mut queue: VecDeque<u32> = VecDeque::with_capacity(csr.nodes());
    let mut plain = f64::MAX;
    let mut ahead = f64::MAX;
    let mut settled = 0;
    for round in 0..rounds {
        let source = ((round * 7919) % csr.nodes()) as u32;
        let at = Instant::now();
        let count = bfs(csr, source, &mut seen, &mut queue);
        plain = plain.min(at.elapsed().as_secs_f64());
        assert!(
            settled == 0 || settled == count,
            "a round settled differently"
        );
        settled = count;
        let at = Instant::now();
        let again = bfs_prefetching(csr, source, &mut seen, &mut queue);
        ahead = ahead.min(at.elapsed().as_secs_f64());
        assert_eq!(settled, again, "the two searches settled different nodes");
    }
    println!(
        "{what:>22} {:>12} {:>12} {:>9} {plain:>10.4} {ahead:>10.4} {:>7.1}% {:>7}",
        csr.nodes(),
        csr.targets.len(),
        format!("{} MiB", working_set(csr.nodes(), csr.targets.len()) >> 20),
        100.0 * (plain - ahead) / plain,
        if worth_prefetching(csr.nodes(), csr.targets.len()) {
            "yes"
        } else {
            "no"
        },
    );
}

fn main() {
    // what the gating column is measured against
    println!("last level cache: {} MiB", last_level_cache() >> 20);
    println!(
        "{:>22} {:>12} {:>12} {:>9} {:>10} {:>10} {:>8} {:>7}",
        "instance", "nodes", "arcs", "touched", "plain", "prefetch", "saved", "gated"
    );
    let sides: Vec<usize> = std::env::var("TOOLBOX_SIDES").map_or_else(
        |_| vec![512, 1024, 2048, 4096],
        |given| {
            given
                .split(',')
                .map(|side| side.parse().expect("a grid side"))
                .collect()
        },
    );
    for side in sides {
        let (edges, nodes) = grid(side);
        let csr = Csr::of(&edges, nodes);
        time(&format!("grid {side}x{side}"), &csr, 5);
    }

    if let Some(path) = args().nth(1) {
        let input: Vec<InputEdge<u32>> = io::read_edges_from_file(&path);
        let nodes = 1 + input
            .iter()
            .map(|edge| edge.source.max(edge.target))
            .max()
            .expect("an empty graph");
        let edges: Vec<(u32, u32)> = input
            .iter()
            .map(|edge| (edge.source as u32, edge.target as u32))
            .collect();
        drop(input);
        let csr = Csr::of(&edges, nodes);
        drop(edges);
        time("road network", &csr, 5);
    }
}
