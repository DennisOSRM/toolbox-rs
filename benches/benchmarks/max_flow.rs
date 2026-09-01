//! Dinic against push-relabel, on the shape of graph an inertial flow cut is
//! asked for.
//!
//! A cell of a road network is close to planar and thin: degrees are small, the
//! source and the sink are whole ranks of nodes contracted into one, and the
//! cut is narrow. The grid is that shape. The layered graph is the other end of
//! it, where the cut is wide and the search has to work for it.
use criterion::{BenchmarkId, Criterion, criterion_group};
use rand::{RngExt, SeedableRng, prelude::StdRng};
use std::hint::black_box;
use std::sync::{Arc, atomic::AtomicI32};
use toolbox_rs::{
    dinic::Dinic,
    edge::InputEdge,
    max_flow::{MaxFlow, ResidualEdgeData},
    push_relabel::PushRelabel,
};

/// A grid with the left rank contracted into the source and the right into the
/// sink, which is an inertial flow cut of a square cell.
fn grid(side: usize, rng: &mut StdRng) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
    let source = 0;
    let sink = 1;
    let node = |row: usize, column: usize| 2 + row * side + column;
    let mut edges = Vec::new();
    for row in 0..side {
        for column in 0..side {
            let mut capacity = || ResidualEdgeData::new(rng.random_range(1..=4));
            if column + 1 < side {
                edges.push(InputEdge::new(
                    node(row, column),
                    node(row, column + 1),
                    capacity(),
                ));
                edges.push(InputEdge::new(
                    node(row, column + 1),
                    node(row, column),
                    capacity(),
                ));
            }
            if row + 1 < side {
                edges.push(InputEdge::new(
                    node(row, column),
                    node(row + 1, column),
                    capacity(),
                ));
                edges.push(InputEdge::new(
                    node(row + 1, column),
                    node(row, column),
                    capacity(),
                ));
            }
        }
        edges.push(InputEdge::new(
            source,
            node(row, 0),
            ResidualEdgeData::new(i32::MAX / 4),
        ));
        edges.push(InputEdge::new(
            node(row, side - 1),
            sink,
            ResidualEdgeData::new(i32::MAX / 4),
        ));
    }
    (edges, source, sink)
}

/// A dense layered graph, where the cut is wide.
fn layered(
    width: usize,
    depth: usize,
    rng: &mut StdRng,
) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
    let source = 0;
    let sink = 1 + width * depth;
    let node = |layer: usize, index: usize| 1 + layer * width + index;
    let mut edges = Vec::new();
    for index in 0..width {
        edges.push(InputEdge::new(
            source,
            node(0, index),
            ResidualEdgeData::new(rng.random_range(1..=8)),
        ));
        edges.push(InputEdge::new(
            node(depth - 1, index),
            sink,
            ResidualEdgeData::new(rng.random_range(1..=8)),
        ));
    }
    for layer in 0..depth - 1 {
        for index in 0..width {
            for other in 0..width {
                if rng.random_range(0..100) < 40 {
                    edges.push(InputEdge::new(
                        node(layer, index),
                        node(layer + 1, other),
                        ResidualEdgeData::new(rng.random_range(1..=5)),
                    ));
                }
            }
        }
    }
    (edges, source, sink)
}

fn cuts(c: &mut Criterion) {
    let mut group = c.benchmark_group("min_cut");

    for side in [16_usize, 32, 64, 96] {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let (edges, source, sink) = grid(side, &mut rng);
        group.bench_with_input(BenchmarkId::new("dinic/grid", side), &side, |b, _| {
            b.iter_with_setup(
                || Dinic::from_edge_list(edges.clone(), source, sink),
                |mut solver| {
                    solver.run();
                    black_box(solver.max_flow())
                },
            );
        });
        group.bench_with_input(
            BenchmarkId::new("push_relabel/grid", side),
            &side,
            |b, _| {
                b.iter_with_setup(
                    || PushRelabel::from_edge_list(edges.clone(), source, sink),
                    |mut solver| {
                        solver.run();
                        black_box(solver.max_flow())
                    },
                );
            },
        );
    }

    for width in [16_usize, 32, 64] {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let (edges, source, sink) = layered(width, 8, &mut rng);
        group.bench_with_input(BenchmarkId::new("dinic/layered", width), &width, |b, _| {
            b.iter_with_setup(
                || Dinic::from_edge_list(edges.clone(), source, sink),
                |mut solver| {
                    solver.run();
                    black_box(solver.max_flow())
                },
            );
        });
        group.bench_with_input(
            BenchmarkId::new("push_relabel/layered", width),
            &width,
            |b, _| {
                b.iter_with_setup(
                    || PushRelabel::from_edge_list(edges.clone(), source, sink),
                    |mut solver| {
                        solver.run();
                        black_box(solver.max_flow())
                    },
                );
            },
        );
    }
    group.finish();

    // Construction and run together, which is what a caller really pays. The
    // clone of the edge list is the setup and is not timed; building the
    // residual graph is, and push-relabel pays for pairing the arcs there while
    // dinic looks a pair up per push instead.
    let mut whole = c.benchmark_group("min_cut_built");
    for side in [32_usize, 64, 96] {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let (edges, source, sink) = grid(side, &mut rng);
        whole.bench_with_input(BenchmarkId::new("dinic/grid", side), &side, |b, _| {
            b.iter_with_setup(
                || edges.clone(),
                |list| {
                    let mut solver = Dinic::from_edge_list(list, source, sink);
                    solver.run();
                    black_box(solver.max_flow())
                },
            );
        });
        whole.bench_with_input(
            BenchmarkId::new("push_relabel/grid", side),
            &side,
            |b, _| {
                b.iter_with_setup(
                    || edges.clone(),
                    |list| {
                        let mut solver = PushRelabel::from_edge_list(list, source, sink);
                        solver.run();
                        black_box(solver.max_flow())
                    },
                );
            },
        );
    }
    whole.finish();

    // With an upper bound the solver may give up on, which is how inertial flow
    // uses it: four axes race and each drops out as soon as it is beaten. The
    // bound is set well under the real cut, so both give up rather than finish.
    let mut bounded = c.benchmark_group("min_cut_bounded");
    for side in [32_usize, 64, 96] {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let (edges, source, sink) = grid(side, &mut rng);
        bounded.bench_with_input(BenchmarkId::new("dinic/grid", side), &side, |b, _| {
            b.iter_with_setup(
                || Dinic::from_edge_list(edges.clone(), source, sink),
                |mut solver| {
                    solver.run_with_upper_bound(Arc::new(AtomicI32::new(4)));
                    black_box(solver.max_flow())
                },
            );
        });
        bounded.bench_with_input(
            BenchmarkId::new("push_relabel/grid", side),
            &side,
            |b, _| {
                b.iter_with_setup(
                    || PushRelabel::from_edge_list(edges.clone(), source, sink),
                    |mut solver| {
                        solver.run_with_upper_bound(Arc::new(AtomicI32::new(4)));
                        black_box(solver.max_flow())
                    },
                );
            },
        );
    }
    bounded.finish();
}

criterion_group!(max_flow, cuts);
