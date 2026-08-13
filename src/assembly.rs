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
use crate::level_directory::{CellId, LevelDirectory};

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
}
