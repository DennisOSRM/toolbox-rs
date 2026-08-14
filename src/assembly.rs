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

use crate::{edge::TrivialEdge, level_directory::CellId};

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
