//! Assembling the cells of a bisection into levels of a wanted size.
//!
//! # The problem
//!
//! A recursive bisection that stops once a cell is small enough leaves a tree
//! whose leaves are far finer than any level one wants to route on. Turning it
//! into levels of, say, 50, 250 and 1000 nodes means choosing per level which
//! cells to keep whole and which to break into their children. A choice is a
//! set of tree nodes covering every leaf once, the levels have to nest, and the
//! cost of a level is the number of arcs leaving its cells.
//!
//! # Is it an optimisation problem
//!
//! It is, but only once cells may be grouped freely. Along the tree it is not.
//!
//! The arcs inside a cell are the arcs inside its two children plus the arcs
//! between them, so a parent always keeps at least as many arcs inside it as
//! its children do together. Keeping a cell whole is therefore never worse than
//! splitting it, and the best level under a size bound is simply the highest
//! cell that still fits. That is what [`assemble`] does, in one walk per level,
//! and no search is involved. The levels nest for free: a cell that fits under
//! one bound fits under every larger one, so the frontier only ever moves up.
//!
//! What the tree cannot do is undo a split. Two cells that the bisection put on
//! opposite sides near the root stay apart on every level, however few arcs
//! separate them, and a cell whose sibling is large ends up far below the size
//! that was asked for. Repairing that means grouping any two neighbouring cells
//! rather than only siblings, which is graph partitioning under a size bound on
//! the graph whose nodes are the cells of the bisection and whose arc weights
//! count the arcs running between them. That problem is NP-hard, and the usual
//! attack is the coarsening phase of a multilevel partitioner: repeatedly merge
//! the neighbouring pair sharing the most arcs while the size bound allows it.
//! It can beat the tree, at the price of a heuristic with no bound on how far
//! off it lands. [`assemble`] does not do it, and [`Tree`] says what it needs.
use crate::{
    edge::TrivialEdge,
    level_directory::{CellId, LevelDirectory},
};

/// A cell of the bisection, i.e. a node of the tree it left behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    /// the two cells a cut left behind, and none for a cell that was not cut
    pub children: Option<(usize, usize)>,
    /// how many nodes of the graph the cell holds
    pub size: usize,
}

/// The tree a recursive bisection leaves behind.
///
/// Assembling along this tree can only group cells that the bisection kept
/// together. Grouping any two neighbouring cells instead needs the graph on the
/// cells of the bisection, with an arc weighted by how many arcs of the graph
/// run between two cells, and a partitioner over it.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// the cells, every child before its parent
    nodes: Vec<Node>,
    /// the cell of the tree that each node of the graph ended up in
    leaf_of_node: Vec<usize>,
}

impl Tree {
    /// # Panics
    ///
    /// Panics if a child comes after its parent, if the sizes of two children
    /// do not add up to their parent, or if a node of the graph sits in a cell
    /// that was cut further.
    #[must_use]
    pub fn new(nodes: Vec<Node>, leaf_of_node: Vec<usize>) -> Self {
        let tree = Self {
            nodes,
            leaf_of_node,
        };
        assert!(tree.is_consistent(), "the tree does not hold together");
        tree
    }

    fn is_consistent(&self) -> bool {
        for (index, node) in self.nodes.iter().enumerate() {
            let Some((left, right)) = node.children else {
                continue;
            };
            if left >= index || right >= index {
                return false;
            }
            if self.nodes[left].size + self.nodes[right].size != node.size {
                return false;
            }
        }
        self.leaf_of_node
            .iter()
            .all(|&leaf| leaf < self.nodes.len() && self.nodes[leaf].children.is_none())
    }

    /// The cell that holds the whole graph, which the tree grew from.
    #[must_use]
    pub fn root(&self) -> usize {
        self.nodes.len() - 1
    }

    #[must_use]
    pub fn number_of_cells(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn number_of_nodes(&self) -> usize {
        self.leaf_of_node.len()
    }

    /// The cells the given one was cut into, from the finest up, so that a walk
    /// from the front settles a cell before the one holding it.
    fn subtree_of(&self, root: usize) -> Vec<usize> {
        let mut order = Vec::new();
        let mut stack = vec![root];
        while let Some(index) = stack.pop() {
            order.push(index);
            if let Some((left, right)) = self.nodes[index].children {
                stack.push(left);
                stack.push(right);
            }
        }
        order
    }
}

/// Assembles the cells of a bisection into one level per wanted size.
///
/// A cell of a level is the highest cell of the bisection that still fits into
/// the size of that level, which is what keeps the most arcs inside it. A cell
/// that the bisection did not cut is kept whether it fits or not, as there is
/// nothing left to break it into.
///
/// `sizes` is read from the lowest level up.
///
/// # Panics
///
/// Panics if `sizes` is empty or does not grow, as a level has to be at least
/// as coarse as the one below it.
#[must_use]
pub fn assemble(tree: &Tree, sizes: &[usize]) -> LevelDirectory {
    assert!(!sizes.is_empty(), "a hierarchy needs a level");
    assert!(
        sizes.windows(2).all(|pair| pair[0] <= pair[1]),
        "the levels have to grow"
    );

    let mut base = Vec::new();
    let mut parents: Vec<Vec<CellId>> = Vec::new();
    // the cell of the tree that each cell of the level below stands for
    let mut below: Vec<usize> = Vec::new();

    for (level, &size) in sizes.iter().enumerate() {
        // walk down from the root and stop on the first cell that fits
        let mut cell_of_tree_node = vec![CellId::MAX; tree.number_of_cells()];
        let mut frontier = Vec::new();
        let mut stack = vec![tree.root()];
        while let Some(index) = stack.pop() {
            let node = tree.nodes[index];
            match node.children {
                Some((left, right)) if node.size > size => {
                    stack.push(left);
                    stack.push(right);
                }
                _ => {
                    // the whole subtree of this cell belongs to it
                    let cell = CellId::try_from(frontier.len()).expect("too many cells on a level");
                    for descendant in tree.subtree_of(index) {
                        cell_of_tree_node[descendant] = cell;
                    }
                    frontier.push(index);
                }
            }
        }

        if level == 0 {
            base = tree
                .leaf_of_node
                .iter()
                .map(|&leaf| cell_of_tree_node[leaf])
                .collect();
        } else {
            // every cell of the level below lies in exactly one of this level
            parents.push(
                below
                    .iter()
                    .map(|&index| cell_of_tree_node[index])
                    .collect(),
            );
        }
        below = frontier;
    }

    LevelDirectory::new(base, parents)
}

/// The graph on the cells a bisection left behind: how large each cell is, and
/// how many arcs of the graph run between two of them.
///
/// This is what an assembly needs that the [`Tree`] does not carry. Merging two
/// cells that share an arc keeps the result in one piece as long as both of
/// them were, which is what a cell has to be for a search to cross it without
/// leaving it.
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
    fn arcs(&self) -> Vec<(usize, usize, usize)> {
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
/// Panics if `sizes` is empty or does not grow.
#[must_use]
pub fn assemble_connected(
    graph: &CellGraph,
    cell_of_node: &[CellId],
    sizes: &[usize],
) -> LevelDirectory {
    assert!(!sizes.is_empty(), "a hierarchy needs a level");
    assert!(
        sizes.windows(2).all(|pair| pair[0] <= pair[1]),
        "the levels have to grow"
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
        current = contract(&current, &merged);
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
/// joins them, directly or through other nodes of it.
#[must_use]
pub fn fragments(nodes: usize, arcs: &[TrivialEdge], cell_of_node: &[CellId]) -> Vec<CellId> {
    let mut union = crate::union_find::UnionFind::new(nodes);
    for arc in arcs {
        if cell_of_node[arc.source] == cell_of_node[arc.target] {
            union.union(arc.source, arc.target);
        }
    }

    // number the pieces in the order their nodes come
    let mut piece_of_root = vec![CellId::MAX; nodes];
    let mut pieces = 0;
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
    // than staying inside it. The ends are put in order rather than the arc
    // being taken only when they already are: an arc that runs from a higher
    // numbered cell to a lower one is an arc between them all the same, and
    // dropping it leaves cells looking like they have no neighbour. An arc that
    // the list holds in both directions is counted twice, which is the same for
    // every pair and so does not change which of them is merged first.
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

    /// A tree over eight nodes of the graph, cut down to cells of one:
    /// ```text
    ///            14
    ///        /        \
    ///      12          13
    ///     /  \        /  \
    ///    8    9     10    11
    ///   / \  / \   /  \  /  \
    ///   0 1  2 3   4  5  6  7
    /// ```
    fn balanced_tree() -> Tree {
        let leaf = |size| Node {
            children: None,
            size,
        };
        let inner = |left: usize, right: usize, size| Node {
            children: Some((left, right)),
            size,
        };
        Tree::new(
            vec![
                leaf(1),
                leaf(1),
                leaf(1),
                leaf(1),
                leaf(1),
                leaf(1),
                leaf(1),
                leaf(1),
                inner(0, 1, 2),
                inner(2, 3, 2),
                inner(4, 5, 2),
                inner(6, 7, 2),
                inner(8, 9, 4),
                inner(10, 11, 4),
                inner(12, 13, 8),
            ],
            (0..8).collect(),
        )
    }

    #[test]
    fn a_level_keeps_the_highest_cell_that_fits() {
        let tree = balanced_tree();
        let directory = assemble(&tree, &[2]);
        assert_eq!(directory.cells_on_level(0), 4);
        // the pairs the tree put together end up together
        for pair in [(0, 1), (2, 3), (4, 5), (6, 7)] {
            assert!(directory.same_cell(pair.0, pair.1, 0));
        }
        assert!(!directory.same_cell(1, 2, 0));
    }

    #[test]
    fn the_levels_grow_as_the_sizes_do() {
        let tree = balanced_tree();
        let directory = assemble(&tree, &[1, 2, 4, 8]);
        assert_eq!(directory.levels(), 4);
        assert_eq!(directory.cells_on_level(0), 8);
        assert_eq!(directory.cells_on_level(1), 4);
        assert_eq!(directory.cells_on_level(2), 2);
        assert_eq!(directory.cells_on_level(3), 1);
    }

    #[test]
    fn the_levels_nest() {
        let tree = balanced_tree();
        let directory = assemble(&tree, &[1, 2, 4, 8]);
        // two nodes that share a cell keep sharing one further up
        for u in 0..8 {
            for v in 0..8 {
                let meeting = directory.common_level(u, v).expect("a shared root");
                for level in meeting..directory.levels() {
                    assert!(directory.same_cell(u, v, level), "{u} and {v} at {level}");
                }
            }
        }
    }

    #[test]
    fn the_level_two_nodes_meet_on_follows_the_tree() {
        let tree = balanced_tree();
        let directory = assemble(&tree, &[1, 2, 4, 8]);
        assert_eq!(directory.common_level(0, 1), Some(1));
        assert_eq!(directory.common_level(0, 3), Some(2));
        assert_eq!(directory.common_level(0, 7), Some(3));
    }

    #[test]
    fn a_size_below_the_bisection_gives_its_cells() {
        let tree = balanced_tree();
        // the bisection stopped at one node per cell, so nothing is finer
        let directory = assemble(&tree, &[1]);
        assert_eq!(directory.cells_on_level(0), 8);
    }

    /// A cell whose sibling is large ends up far below the size that was asked
    /// for, which is what the tree cannot repair.
    #[test]
    fn a_cell_can_come_out_far_below_the_size_of_its_level() {
        let leaf = |size| Node {
            children: None,
            size,
        };
        let inner = |left: usize, right: usize, size| Node {
            children: Some((left, right)),
            size,
        };
        // one cell of 9 next to one of 1, so a level of 9 cannot hold both
        let tree = Tree::new(
            vec![leaf(9), leaf(1), inner(0, 1, 10)],
            std::iter::repeat_n(0, 9)
                .chain(std::iter::once(1))
                .collect(),
        );
        let directory = assemble(&tree, &[9]);
        assert_eq!(directory.cells_on_level(0), 2);
        // the node on its own is a cell of one, nowhere near the nine asked for
        assert!(!directory.same_cell(0, 9, 0));
    }

    #[test]
    fn an_uneven_tree_is_walked_all_the_same() {
        let leaf = |size| Node {
            children: None,
            size,
        };
        let inner = |left: usize, right: usize, size| Node {
            children: Some((left, right)),
            size,
        };
        //      4
        //     / \
        //    3   2      (a cell of 2 that was not cut)
        //   / \
        //   0  1
        let tree = Tree::new(
            vec![leaf(1), leaf(1), leaf(2), inner(0, 1, 2), inner(3, 2, 4)],
            vec![0, 1, 2, 2],
        );
        let directory = assemble(&tree, &[2, 4]);
        assert_eq!(directory.cells_on_level(0), 2);
        assert_eq!(directory.cells_on_level(1), 1);
        assert!(directory.same_cell(2, 3, 0), "the cell of two stays whole");
        assert!(!directory.same_cell(0, 2, 0));
        assert!(directory.same_cell(0, 2, 1));
    }

    #[test]
    #[should_panic(expected = "the levels have to grow")]
    fn a_level_finer_than_the_one_below_is_caught() {
        let _ = assemble(&balanced_tree(), &[4, 2]);
    }

    #[test]
    #[should_panic(expected = "the tree does not hold together")]
    fn a_parent_that_does_not_hold_its_children_is_caught() {
        let tree = vec![
            Node {
                children: None,
                size: 1,
            },
            Node {
                children: None,
                size: 1,
            },
            Node {
                children: Some((0, 1)),
                size: 3,
            },
        ];
        let _ = Tree::new(tree, vec![0, 1]);
    }

    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// Grows a bisection tree by cutting cells until they are small enough,
    /// which is the shape the partitioner leaves behind.
    fn random_tree(rng: &mut StdRng, nodes: usize, stop_at: usize) -> Tree {
        // cells in creation order, a parent before its children
        let mut sizes = vec![nodes];
        let mut children: Vec<Option<(usize, usize)>> = vec![None];
        let mut leaf_of_node = vec![0_usize; nodes];
        let mut members = vec![(0..nodes).collect::<Vec<_>>()];

        let mut index = 0;
        while index < sizes.len() {
            if sizes[index] > stop_at {
                let own = std::mem::take(&mut members[index]);
                // cut somewhere, but never so that a side comes out empty
                let at = rng.random_range(1..own.len());
                let (left, right) = own.split_at(at);
                let (left, right) = (left.to_vec(), right.to_vec());

                children[index] = Some((sizes.len(), sizes.len() + 1));
                for half in [left, right] {
                    sizes.push(half.len());
                    children.push(None);
                    for &node in &half {
                        leaf_of_node[node] = members.len();
                    }
                    members.push(half);
                }
            }
            index += 1;
        }

        // the assembly wants every child before its parent
        let last = sizes.len() - 1;
        let flip = |index: usize| last - index;
        let nodes = sizes
            .iter()
            .zip(&children)
            .map(|(&size, children)| Node {
                size,
                children: children.map(|(left, right)| (flip(left), flip(right))),
            })
            .rev()
            .collect();
        Tree::new(nodes, leaf_of_node.iter().map(|&leaf| flip(leaf)).collect())
    }

    #[test]
    fn an_assembled_hierarchy_nests_and_is_answerable() {
        let mut rng = StdRng::seed_from_u64(0xA55E);
        for round in 0..10 {
            let tree = random_tree(&mut rng, 60 + round * 7, 3);
            let directory = assemble(&tree, &[4, 10, 25, 200]);

            assert_eq!(directory.number_of_nodes(), tree.number_of_nodes());
            assert_eq!(directory.levels(), 4);

            for u in 0..directory.number_of_nodes() {
                for v in 0..directory.number_of_nodes() {
                    // two nodes that meet stay together above that level
                    let meeting = directory.common_level(u, v);
                    for level in 0..directory.levels() {
                        let same = directory.same_cell(u, v, level);
                        assert_eq!(same, meeting.is_some_and(|first| level >= first));
                    }
                }
            }
            // a size holding the whole graph puts everything into one cell
            assert_eq!(directory.cells_on_level(3), 1);
        }
    }

    #[test]
    fn no_cell_of_a_level_is_larger_than_the_level_allows() {
        let mut rng = StdRng::seed_from_u64(0xB0A7);
        for round in 0..10 {
            let tree = random_tree(&mut rng, 80 + round * 5, 2);
            let sizes = [8, 20, 60];
            let directory = assemble(&tree, &sizes);

            for (level, &size) in sizes.iter().enumerate() {
                let mut count = vec![0_usize; directory.cells_on_level(level)];
                for node in 0..directory.number_of_nodes() {
                    count[directory.cell_of(node, level) as usize] += 1;
                }
                // a cell only grows past its size when the bisection left one
                // that large and nothing can break it further
                for (cell, &held) in count.iter().enumerate() {
                    assert!(
                        held <= size.max(2),
                        "cell {cell} of level {level} holds {held}"
                    );
                }
            }
        }
    }

    /// Whether every cell of every level is in one piece, checked by walking
    /// the graph of the cells without leaving the cell of the level.
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

    /// What the assembly along the tree cannot promise, for contrast: it never
    /// sees which cells are neighbours, so a cell of it can fall apart.
    #[test]
    fn the_assembly_along_the_tree_makes_no_such_promise() {
        // two cells that share no arc, put together by the tree because they
        // happen to be siblings
        let graph = CellGraph::new(vec![1, 1], &[]);
        let cell_of_node = vec![0, 1];
        let tree = Tree::new(
            vec![
                Node {
                    children: None,
                    size: 1,
                },
                Node {
                    children: None,
                    size: 1,
                },
                Node {
                    children: Some((0, 1)),
                    size: 2,
                },
            ],
            vec![0, 1],
        );

        let along_the_tree = assemble(&tree, &[2]);
        assert!(
            along_the_tree.same_cell(0, 1, 0),
            "the tree puts them together"
        );
        assert!(
            !every_cell_is_connected(&graph, &cell_of_node, &along_the_tree),
            "and the cell it makes is in two pieces"
        );

        // merging along arcs leaves them apart, as there is no arc to merge on
        let merged = assemble_connected(&graph, &cell_of_node, &[2]);
        assert!(!merged.same_cell(0, 1, 0));
        assert!(every_cell_is_connected(&graph, &cell_of_node, &merged));
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

    fn edge(source: usize, target: usize) -> TrivialEdge {
        TrivialEdge { source, target }
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
}
