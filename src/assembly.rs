//! Assembling the cells of a partition into levels of a wanted size.
//!
//! A recursive bisection that stops once a cell is small enough leaves cells
//! far finer than any level one wants to route on. Building the levels one
//! actually asked for, say of 50, 250 and 1000 nodes, means merging those cells
//! until each is as large as the level wants, and doing it so that a cell of
//! the result still holds together.
//!
//! That last part is why this is not simply a matter of counting: a search that
//! crosses a cell has to be able to do it without leaving the cell, and a cell
//! that falls into pieces cannot promise that.

use crate::{
    edge::TrivialEdge,
    level_directory::{CellId, LevelDirectory},
};

/// The graph on the cells a partition left behind: how large each cell is, and
/// how many arcs of the graph run between two of them.
///
/// This is what says which cells may be merged, which the cells alone do not.
/// Merging two cells that share an arc keeps the result in one piece as long as
/// both of them were, and that is what a cell has to be for a search to cross
/// it without leaving it.
#[derive(Clone, Debug, Default)]
pub struct CellGraph {
    sizes: Vec<usize>,
    /// the cells next to each one and how many arcs reach them, both ways round
    neighbours: Vec<Vec<(usize, usize)>>,
}

impl CellGraph {
    /// `arcs` holds how many arcs of the graph run between two cells, once per
    /// pair and in either order.
    #[must_use]
    pub fn new(sizes: Vec<usize>, arcs: &[(usize, usize, usize)]) -> Self {
        let mut neighbours = vec![Vec::new(); sizes.len()];
        for &(left, right, weight) in arcs {
            assert!(left != right, "a cell is not next to itself");
            assert!(
                left < sizes.len() && right < sizes.len(),
                "an arc reaches a cell the graph does not have"
            );
            neighbours[left].push((right, weight));
            neighbours[right].push((left, weight));
        }
        Self { sizes, neighbours }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.sizes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    #[must_use]
    pub fn size_of(&self, cell: usize) -> usize {
        self.sizes[cell]
    }

    #[must_use]
    pub fn neighbours_of(&self, cell: usize) -> &[(usize, usize)] {
        &self.neighbours[cell]
    }

    /// The arcs between two cells, each pair once.
    #[must_use]
    pub fn arcs(&self) -> Vec<(usize, usize, usize)> {
        let mut arcs = Vec::new();
        for (left, neighbours) in self.neighbours.iter().enumerate() {
            for &(right, weight) in neighbours {
                if left < right {
                    arcs.push((left, right, weight));
                }
            }
        }
        arcs
    }
}

/// Merges neighbouring cells until none of them can take another without
/// passing `size`, and hands back the cell each one ended up in.
///
/// The pairs are taken by how many arcs run between them, the heaviest first,
/// which is the greedy agglomeration that the coarsening of a multilevel
/// partitioner is built on. Only cells that share an arc are ever merged, so a
/// cell of the result is a union of cells joined along arcs and stays in one
/// piece as long as the cells it was built from were.
#[must_use]
pub fn agglomerate(graph: &CellGraph, size: usize) -> Vec<CellId> {
    let mut arcs = graph.arcs();
    // heaviest first, and by cell for a run that does not depend on the order
    // the arcs happened to be collected in
    arcs.sort_unstable_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));

    let mut union = crate::union_find::UnionFind::new(graph.len());
    let mut merged_size = graph.sizes.clone();
    for (left, right, _) in arcs {
        let (left, right) = (union.find(left), union.find(right));
        if left == right || merged_size[left] + merged_size[right] > size {
            continue;
        }
        let total = merged_size[left] + merged_size[right];
        union.union(left, right);
        let root = union.find(left);
        merged_size[root] = total;
    }

    // number the cells that are left, in the order their members appear
    let mut cell_of_root = vec![CellId::MAX; graph.len()];
    let mut cells = 0;
    (0..graph.len())
        .map(|cell| {
            let root = union.find(cell);
            if cell_of_root[root] == CellId::MAX {
                cell_of_root[root] = cells;
                cells += 1;
            }
            cell_of_root[root]
        })
        .collect()
}

/// Draws the cells of a graph together along the given assignment.
#[must_use]
pub fn contract(graph: &CellGraph, of: &[CellId]) -> CellGraph {
    let cells = of.iter().copied().max().map_or(0, |cell| cell as usize + 1);
    let mut sizes = vec![0; cells];
    for (cell, size) in graph.sizes.iter().enumerate() {
        sizes[of[cell] as usize] += size;
    }

    // the arcs between two cells of the result are the arcs between the cells
    // they were built from, and the ones inside a cell are gone
    let mut between: rustc_hash::FxHashMap<(usize, usize), usize> =
        rustc_hash::FxHashMap::default();
    for (left, right, weight) in graph.arcs() {
        let (left, right) = (of[left] as usize, of[right] as usize);
        if left == right {
            continue;
        }
        let pair = (left.min(right), left.max(right));
        *between.entry(pair).or_insert(0) += weight;
    }

    let mut arcs = between
        .into_iter()
        .map(|((left, right), weight)| (left, right, weight))
        .collect::<Vec<_>>();
    arcs.sort_unstable();
    CellGraph::new(sizes, &arcs)
}

/// Assembles the levels by merging neighbouring cells, so that every cell of
/// every level is a union of cells joined along arcs.
///
/// `cell_of_node` says which cell of `graph` each node of the graph sits in.
/// `sizes` is read from the lowest level up.
///
/// # Panics
///
/// Panics if `sizes` is empty or holds a level finer than the one below it.
/// Two levels of the same size are allowed: the second is merged out of the
/// first rather than out of the cells the first was merged from, and greedy
/// merging under the same bound can find pairs on the coarser graph that it
/// could not on the finer one.
#[must_use]
pub fn assemble_connected(
    graph: &CellGraph,
    cell_of_node: &[CellId],
    sizes: &[usize],
) -> LevelDirectory {
    assert!(!sizes.is_empty(), "a hierarchy needs a level");
    assert!(
        sizes.windows(2).all(|pair| pair[0] <= pair[1]),
        "a level cannot be finer than the one below it"
    );

    let mut current = graph.clone();
    let mut base = Vec::new();
    let mut parents = Vec::new();
    for (level, &size) in sizes.iter().enumerate() {
        log::debug!(
            "level {level}: merging {} cells joined by {} arcs, up to {size} nodes",
            current.len(),
            current.arcs().len()
        );
        let merged = agglomerate(&current, size);
        if level == 0 {
            base = cell_of_node
                .iter()
                .map(|&cell| merged[cell as usize])
                .collect();
        } else {
            parents.push(merged.clone());
        }
        // the topmost level has nothing built on it, and drawing the graph
        // together once more would walk every arc of it for nobody
        if level + 1 < sizes.len() {
            current = contract(&current, &merged);
        }
    }

    LevelDirectory::new(base, parents)
}

/// Splits the cells of a partition into the pieces they consist of, so that
/// every piece is in one piece.
///
/// A cell that a bisection leaves behind need not hold together: a minimum cut
/// puts everything the source cannot reach on the other side, whether it hangs
/// together with the rest or not. Merging such a cell into a larger one carries
/// the split upwards, so the pieces have to be taken apart before anything is
/// built on top of them.
///
/// Two nodes end up in the same piece exactly when an arc of their own cell
/// joins them, directly or through other nodes of it. The arcs are walked
/// whichever way round they run, as a cell is crossed by a path that may take
/// either.
///
/// # Panics
///
/// Panics if `cell_of_node` does not hold a cell for every node, if an arc
/// reaches past the nodes, or if the graph holds more nodes than a [`CellId`]
/// can number, as every node can end up a piece of its own.
#[must_use]
pub fn fragments(nodes: usize, arcs: &[TrivialEdge], cell_of_node: &[CellId]) -> Vec<CellId> {
    assert_eq!(
        cell_of_node.len(),
        nodes,
        "the partition does not cover the graph"
    );
    assert!(
        nodes <= CellId::MAX as usize,
        "more nodes than pieces can be numbered"
    );

    let mut union = crate::union_find::UnionFind::new(nodes);
    for arc in arcs {
        if cell_of_node[arc.source] == cell_of_node[arc.target] {
            union.union(arc.source, arc.target);
        }
    }

    // number the pieces in the order their nodes come
    let mut piece_of_root = vec![CellId::MAX; nodes];
    let mut pieces: CellId = 0;
    (0..nodes)
        .map(|node| {
            let root = union.find(node);
            if piece_of_root[root] == CellId::MAX {
                piece_of_root[root] = pieces;
                pieces += 1;
            }
            piece_of_root[root]
        })
        .collect()
}

/// Builds the graph on the cells: how large each one is, and how many arcs run
/// between two of them.
///
/// The count is of directed arcs, so a pair joined by a road that runs both
/// ways weighs twice what a pair joined by a one way street does.
#[must_use]
pub fn cell_graph(arcs: &[TrivialEdge], cell_of_node: &[CellId]) -> CellGraph {
    let cells = cell_of_node
        .iter()
        .copied()
        .max()
        .map_or(0, |cell| cell as usize + 1);
    let mut sizes = vec![0; cells];
    for &cell in cell_of_node {
        sizes[cell as usize] += 1;
    }

    // Collect the pairs and count the runs rather than hashing them, as a road
    // network cut into pieces of a dozen nodes has more arcs leaving a piece
    // than staying inside it.
    //
    // The ends are put in order rather than the arc being taken only when they
    // already are: an arc that runs from a higher numbered cell to a lower one
    // is an arc between them all the same, and dropping it leaves cells looking
    // like they have no neighbour at all.
    //
    // What comes out counts directed arcs, so a pair joined by a road that runs
    // both ways counts twice and one joined by a one way street counts once.
    // That is not a uniform doubling of an undirected count, and on a network
    // where one arc in twenty carries no reverse it does move which pair is
    // merged first. It is left that way on purpose: two cells joined by roads
    // that can be driven in both directions are more strongly joined than two
    // held together by a single one way street, and the weight says so.
    let mut pairs = Vec::new();
    for arc in arcs {
        let (left, right) = (cell_of_node[arc.source], cell_of_node[arc.target]);
        if left != right {
            pairs.push((left.min(right), left.max(right)));
        }
    }
    pairs.sort_unstable();

    let mut between = Vec::new();
    let mut run = pairs.first().copied();
    let mut count = 0;
    for pair in &pairs {
        if Some(*pair) == run {
            count += 1;
        } else {
            let (left, right) = run.expect("a run has a pair");
            between.push((left as usize, right as usize, count));
            run = Some(*pair);
            count = 1;
        }
    }
    if let Some((left, right)) = run {
        between.push((left as usize, right as usize, count));
    }

    CellGraph::new(sizes, &between)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    fn every_cell_is_connected(
        graph: &CellGraph,
        cell_of_node: &[CellId],
        directory: &LevelDirectory,
    ) -> bool {
        for level in 0..directory.levels() {
            // the cell of the level that each cell of the graph sits in
            let mut of_cell = vec![CellId::MAX; graph.len()];
            for (node, &cell) in cell_of_node.iter().enumerate() {
                of_cell[cell as usize] = directory.cell_of(node, level);
            }

            let mut seen = vec![false; graph.len()];
            let mut pieces = std::collections::HashMap::new();
            for start in 0..graph.len() {
                if seen[start] || of_cell[start] == CellId::MAX {
                    continue;
                }
                let cell = of_cell[start];
                seen[start] = true;
                let mut stack = vec![start];
                while let Some(current) = stack.pop() {
                    for &(next, _) in graph.neighbours_of(current) {
                        if !seen[next] && of_cell[next] == cell {
                            seen[next] = true;
                            stack.push(next);
                        }
                    }
                }
                // a second walk into the same cell means it fell apart
                if !pieces.insert(cell, start).is_none() {
                    return false;
                }
            }
        }
        true
    }

    /// A ring of eight cells, so that only neighbours share an arc.
    fn ring(cells: usize) -> CellGraph {
        let arcs = (0..cells)
            .map(|cell| (cell, (cell + 1) % cells, 1))
            .collect::<Vec<_>>();
        CellGraph::new(vec![1; cells], &arcs)
    }

    #[test]
    fn merging_follows_the_arcs_between_cells() {
        // a line of four, so a cell of two can only be two that sit next to
        // each other
        let graph = CellGraph::new(vec![1, 1, 1, 1], &[(0, 1, 5), (1, 2, 1), (2, 3, 5)]);
        let merged = agglomerate(&graph, 2);
        assert_eq!(merged[0], merged[1], "the heaviest pair goes together");
        assert_eq!(merged[2], merged[3]);
        assert_ne!(merged[0], merged[2]);
    }

    #[test]
    fn cells_that_share_no_arc_are_left_apart() {
        let graph = CellGraph::new(vec![1, 1, 1], &[]);
        let merged = agglomerate(&graph, 10);
        assert_eq!(merged.len(), 3);
        // nothing to merge along, so nothing is merged however large the size
        assert_ne!(merged[0], merged[1]);
        assert_ne!(merged[1], merged[2]);
    }

    #[test]
    fn a_merge_never_passes_the_size() {
        let graph = ring(8);
        for size in 1..=8 {
            let merged = agglomerate(&graph, size);
            let mut held = vec![0; merged.iter().copied().max().unwrap() as usize + 1];
            for &cell in &merged {
                held[cell as usize] += 1;
            }
            assert!(
                held.iter().all(|&count| count <= size),
                "a cell of {} passed the size {size}",
                held.iter().max().unwrap()
            );
        }
    }

    #[test]
    fn drawing_cells_together_keeps_the_arcs_between_them() {
        let graph = CellGraph::new(vec![1, 1, 1, 1], &[(0, 1, 3), (1, 2, 7), (2, 3, 2)]);
        // put 0 with 1 and 2 with 3
        let contracted = contract(&graph, &[0, 0, 1, 1]);
        assert_eq!(contracted.len(), 2);
        assert_eq!(contracted.size_of(0), 2);
        assert_eq!(contracted.size_of(1), 2);
        // the arc inside a cell is gone, the one between them is kept
        assert_eq!(contracted.neighbours_of(0), &[(1, 7)]);
    }

    #[test]
    fn merging_carries_on_while_there_is_room() {
        let mut rng = StdRng::seed_from_u64(0x57A11);
        for round in 0..10 {
            let cells = 64;
            // a path, so everything can reach everything, with weights that
            // send the order jumping around it
            let arcs = (0..cells - 1)
                .map(|cell| (cell, cell + 1, 1 + rng.random_range(0..50)))
                .collect::<Vec<_>>();
            let graph = CellGraph::new(vec![1; cells], &arcs);

            // a size that holds the whole path has to leave one cell
            let merged = agglomerate(&graph, cells);
            let left = merged.iter().copied().max().unwrap() + 1;
            assert_eq!(
                left, 1,
                "round {round} left {left} cells of a path of {cells}"
            );
        }
    }

    #[test]
    fn merging_fills_the_size_it_is_given() {
        let mut rng = StdRng::seed_from_u64(0xF111);
        let cells = 96;
        let arcs = (0..cells - 1)
            .map(|cell| (cell, cell + 1, 1 + rng.random_range(0..50)))
            .collect::<Vec<_>>();
        let graph = CellGraph::new(vec![1; cells], &arcs);

        // a path cut into cells of twelve leaves eight of them, give or take
        // the ends that cannot grow further
        let merged = agglomerate(&graph, 12);
        let left = merged.iter().copied().max().unwrap() as usize + 1;
        assert!(
            left <= cells / 6,
            "a path of {cells} left {left} cells of at most 12"
        );
    }

    #[test]
    fn every_assembled_cell_is_in_one_piece() {
        let mut rng = StdRng::seed_from_u64(0xC0117);
        for round in 0..20 {
            // a ring with a few extra arcs, so that merging along arcs matters
            let cells = 24 + round;
            let mut arcs = (0..cells)
                .map(|cell| (cell, (cell + 1) % cells, 1 + rng.random_range(0..5)))
                .collect::<Vec<_>>();
            for _ in 0..round {
                let left = rng.random_range(0..cells);
                let right = rng.random_range(0..cells);
                if left != right
                    && !arcs
                        .iter()
                        .any(|&(a, b, _)| (a, b) == (left, right) || (a, b) == (right, left))
                {
                    arcs.push((left, right, 1 + rng.random_range(0..5)));
                }
            }
            let graph = CellGraph::new(vec![1; cells], &arcs);
            let cell_of_node = (0..cells).map(|cell| cell as CellId).collect::<Vec<_>>();

            let directory = assemble_connected(&graph, &cell_of_node, &[2, 5, 12, cells]);
            assert!(
                every_cell_is_connected(&graph, &cell_of_node, &directory),
                "round {round} left a cell in pieces"
            );
        }
    }

    #[test]
    fn two_levels_of_one_size_are_not_a_repeat() {
        // merging a ring of sixteen under a bound of four twice over: the
        // second pass works on the graph the first left, where pairs that were
        // too large before now fit
        let graph = ring(16);
        let cell_of_node = (0..16).map(|cell| cell as CellId).collect::<Vec<_>>();
        let directory = assemble_connected(&graph, &cell_of_node, &[4, 4]);
        assert!(
            directory.cells_on_level(1) <= directory.cells_on_level(0),
            "the second level of the same size left more cells than the first"
        );
    }

    #[test]
    fn the_levels_of_an_agglomeration_nest() {
        let graph = ring(16);
        let cell_of_node = (0..16).map(|cell| cell as CellId).collect::<Vec<_>>();
        let directory = assemble_connected(&graph, &cell_of_node, &[2, 4, 8, 16]);
        for u in 0..16 {
            for v in 0..16 {
                let meeting = directory.common_level(u, v);
                for level in 0..directory.levels() {
                    assert_eq!(
                        directory.same_cell(u, v, level),
                        meeting.is_some_and(|first| level >= first)
                    );
                }
            }
        }
    }

    /// What the pieces are for: taking a partition whose cells fall apart,
    /// splitting them and assembling on top of that leaves every cell of every
    /// level in one piece.
    #[test]
    fn assembling_on_the_pieces_leaves_every_cell_whole() {
        let mut rng = StdRng::seed_from_u64(0xF1A6);
        for round in 0..10 {
            // a line of nodes, so which nodes hang together is plain
            let nodes = 40 + round;
            let mut arcs = Vec::new();
            for node in 0..nodes - 1 {
                arcs.push(edge(node, node + 1));
                arcs.push(edge(node + 1, node));
            }

            // cells that pay no attention to the line, so many fall apart
            let cells = (0..nodes)
                .map(|_| rng.random_range(0..4) as CellId)
                .collect::<Vec<_>>();

            let pieces = fragments(nodes, &arcs, &cells);
            let graph = cell_graph(&arcs, &pieces);
            let directory = assemble_connected(&graph, &pieces, &[3, 9, nodes]);

            // every cell of every level has to be a stretch of the line
            for level in 0..directory.levels() {
                let mut seen = std::collections::HashMap::new();
                let mut previous = None;
                for node in 0..nodes {
                    let cell = directory.cell_of(node, level);
                    if previous != Some(cell) {
                        assert!(
                            seen.insert(cell, node).is_none(),
                            "round {round}: cell {cell} of level {level} comes in pieces"
                        );
                    }
                    previous = Some(cell);
                }
            }
        }
    }

    /// A single pass over the arcs strands cells: a pair skipped because it was
    /// too large at that moment is never looked at again, although both of them
    /// may still have room once the pass is over.
    /// A grid is what a road network looks like from far enough away. The
    /// levels have to keep coarsening on one, and the graph of the cells has to
    /// stay joined up while they do.
    #[test]
    fn a_grid_keeps_coarsening() {
        let side = 64;
        let cells = side * side;
        let mut arcs = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let cell = row * side + column;
                if column + 1 < side {
                    arcs.push((cell, cell + 1, 1));
                }
                if row + 1 < side {
                    arcs.push((cell, cell + side, 1));
                }
            }
        }
        let graph = CellGraph::new(vec![1; cells], &arcs);
        let of_node = (0..cells).map(|cell| cell as CellId).collect::<Vec<_>>();

        let sizes = [4, 16, 64, 256, 1024, 4096];
        let directory = assemble_connected(&graph, &of_node, &sizes);
        for (level, &size) in sizes.iter().enumerate() {
            let held = directory.cells_on_level(level);
            let ideal = cells.div_ceil(size);
            assert!(
                held <= ideal * 4,
                "level {level} of {size} left {held} cells where {ideal} would do"
            );
        }
    }

    fn edge(source: usize, target: usize) -> TrivialEdge {
        TrivialEdge { source, target }
    }

    #[test]
    #[should_panic(expected = "the partition does not cover the graph")]
    fn a_partition_that_leaves_a_node_out_is_caught() {
        let _ = fragments(3, &[edge(0, 1)], &[0, 0]);
    }

    #[test]
    fn the_graph_on_the_cells_counts_the_arcs_between_them() {
        // two cells of two, joined by two arcs
        let arcs = [
            edge(0, 1),
            edge(1, 0),
            edge(2, 3),
            edge(3, 2),
            edge(1, 2),
            edge(2, 1),
            edge(0, 3),
            edge(3, 0),
        ];
        let graph = cell_graph(&arcs, &[0, 0, 1, 1]);
        assert_eq!(graph.len(), 2);
        assert_eq!(graph.size_of(0), 2);
        assert_eq!(graph.size_of(1), 2);
        // The arcs inside a cell are not counted. The two between them are, and
        // the list holds both of their directions, so they come to four.
        assert_eq!(graph.neighbours_of(0), &[(1, 4)]);
    }

    #[test]
    fn cells_with_nothing_between_them_are_no_neighbours() {
        let arcs = [edge(0, 1), edge(1, 0)];
        let graph = cell_graph(&arcs, &[0, 0, 1]);
        assert_eq!(graph.len(), 2);
        assert!(graph.neighbours_of(0).is_empty());
        assert!(graph.neighbours_of(1).is_empty());
    }

    /// An arc is an arc between two cells whichever way round its ends are
    /// numbered. Taking only the ones that happen to run from a lower numbered
    /// cell to a higher one leaves cells looking like they have no neighbour,
    /// and a cell with no neighbour can never be merged into anything.
    #[test]
    fn an_arc_counts_whichever_way_round_it_runs() {
        // one arc, and it runs from the higher numbered cell to the lower one
        let arcs = [edge(1, 0)];
        let graph = cell_graph(&arcs, &[0, 1]);
        assert_eq!(graph.len(), 2);
        assert_eq!(graph.neighbours_of(0), &[(1, 1)]);
        assert_eq!(graph.neighbours_of(1), &[(0, 1)]);
    }

    #[test]
    fn a_list_that_holds_both_directions_joins_the_same_cells() {
        let one_way = cell_graph(&[edge(0, 1)], &[0, 1]);
        let both_ways = cell_graph(&[edge(0, 1), edge(1, 0)], &[0, 1]);
        // the weight doubles, which is the same for every pair, and the cells
        // are neighbours either way
        assert_eq!(one_way.neighbours_of(0), &[(1, 1)]);
        assert_eq!(both_ways.neighbours_of(0), &[(1, 2)]);
    }

    /// A cell graph built from a connected graph has to be connected too, or
    /// the merging has nothing to work with.
    #[test]
    fn a_connected_graph_gives_a_connected_cell_graph() {
        // a path whose cells are numbered so that arcs run both up and down
        let nodes = 12;
        let arcs = (0..nodes - 1)
            .map(|node| edge(node, node + 1))
            .collect::<Vec<_>>();
        let cells = (0..nodes)
            .map(|node| ((nodes - node) % 4) as CellId)
            .collect::<Vec<_>>();

        let graph = cell_graph(&arcs, &cells);
        let mut union = crate::union_find::UnionFind::new(graph.len());
        for (left, right, _) in graph.arcs() {
            union.union(left, right);
        }
        assert_eq!(union.number_of_sets(), 1, "the cell graph fell apart");
        for cell in 0..graph.len() {
            assert!(
                !graph.neighbours_of(cell).is_empty(),
                "cell {cell} has no neighbour"
            );
        }
    }

    #[test]
    fn a_cell_in_two_pieces_is_taken_apart() {
        // one cell of four nodes, but only 0-1 and 2-3 are joined
        let arcs = [edge(0, 1), edge(1, 0), edge(2, 3), edge(3, 2)];
        let pieces = fragments(4, &arcs, &[0, 0, 0, 0]);
        assert_eq!(pieces[0], pieces[1]);
        assert_eq!(pieces[2], pieces[3]);
        assert_ne!(pieces[0], pieces[2], "the two halves are not joined");
    }

    #[test]
    fn an_arc_leaving_a_cell_does_not_join_its_pieces() {
        // 0 and 2 are joined by an arc, but they sit in different cells
        let arcs = [edge(0, 2), edge(2, 0), edge(1, 3), edge(3, 1)];
        let pieces = fragments(4, &arcs, &[0, 1, 0, 1]);
        assert_eq!(pieces[0], pieces[2]);
        assert_eq!(pieces[1], pieces[3]);
        assert_ne!(pieces[0], pieces[1]);
    }

    #[test]
    fn a_node_no_arc_of_its_cell_reaches_is_a_piece_of_its_own() {
        let arcs = [edge(0, 1), edge(1, 0)];
        let pieces = fragments(3, &arcs, &[0, 0, 0]);
        assert_eq!(pieces[0], pieces[1]);
        assert_ne!(pieces[2], pieces[0]);
    }

    #[test]
    fn a_cell_that_holds_together_is_left_whole() {
        let arcs = [edge(0, 1), edge(1, 0), edge(1, 2), edge(2, 1)];
        let pieces = fragments(3, &arcs, &[0, 0, 0]);
        assert_eq!(pieces, vec![0, 0, 0]);
    }

    #[test]
    fn a_one_way_arc_joins_a_cell_all_the_same() {
        // 0 -> 1 and nothing back, which still leaves the cell in one piece as
        // far as the assembly is concerned
        let arcs = [edge(0, 1)];
        let pieces = fragments(2, &arcs, &[0, 0]);
        assert_eq!(pieces, vec![0, 0]);
    }

    #[test]
    fn the_pieces_are_numbered_from_zero_in_the_order_of_their_nodes() {
        // three cells, the middle one in two pieces
        let arcs = [edge(0, 1), edge(3, 4)];
        let pieces = fragments(5, &arcs, &[0, 0, 1, 1, 1]);
        assert_eq!(pieces, vec![0, 0, 1, 2, 2]);
    }
}
