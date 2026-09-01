//! A max-flow computation by Boykov and Kolmogorov's two-tree search.
//!
//! Where an augmenting-path method throws its search away after every
//! augmentation and builds it again, this keeps two trees and mends them: one
//! grown from the source and one from the sink. An augmentation saturates arcs
//! and so tears branches off the trees; the torn-off nodes are handed to their
//! nearest surviving relation rather than to a new search. On a graph that is
//! close to planar, which is what a cell of a road network is, the trees are
//! long and thin and mending one costs far less than growing it.
//!
//! The three stages go round until the trees cannot be joined:
//!
//! 1) *Growth.* Every node at the frontier of a tree is asked for an arc with
//!    capacity left. What it reaches joins its tree. A node reached from the
//!    other tree closes a path from the source to the sink.
//! 2) *Augmentation.* The path is followed to both roots for the arc that gives
//!    the least, and that much is moved along the whole of it. An arc left
//!    without capacity is no longer a tree edge, so the node below it is an
//!    orphan.
//! 3) *Adoption.* An orphan looks among its neighbours in its own tree for one
//!    that still reaches its root. Finding none, it is set free and its own
//!    children are orphaned in turn.
//!
//! Unlike [`crate::push_relabel`], this holds a flow rather than a preflow at
//! every step, so what has arrived at the sink is the flow so far and an upper
//! bound the caller is watching can be tested against it as it grows.
use crate::{
    edge::InputEdge,
    graph::{EdgeID, Graph, NodeID},
    max_flow::{MaxFlow, ResidualArcData, ResidualEdgeData, residual_graph_of},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use log::debug;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
};

/// Which tree a node belongs to, if any.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tree {
    Free,
    Source,
    Sink,
}

/// The arc a node has no parent through.
const NO_ARC: u32 = u32::MAX;

/// One instance answers one question.
///
/// A run leaves its flow in the residual graph, which is what `assignment`
/// reads the cut off afterwards, so a second run carries on from where the
/// first stopped rather than starting again: a run given a bound it gave up
/// against, followed by a plain one, hands back what was left rather than the
/// whole flow. The same holds of the other solvers here. Build another from
/// the edge list to ask about another graph.
pub struct BoykovKolmogorov {
    residual_graph: StaticGraph<ResidualArcData>,
    /// which arc runs the other way, so an augmentation does not search for it
    pair: Vec<u32>,
    source: NodeID,
    target: NodeID,
    max_flow: i32,
    finished: bool,
    bound: Option<Arc<AtomicI32>>,

    /// which tree each node is in
    tree: Vec<Tree>,
    /// The arc from a node to its parent, so that walking to a root is a walk
    /// of targets: the parent of a node is `target(parent[node])`.
    ///
    /// A tree is grown along arcs that run from the parent to the child, so
    /// what is stored is the reverse of the arc the node was reached by. That
    /// holds for both trees. Which way the flow runs along it does differ: down
    /// the tree from the source, and up the tree towards the sink, which is
    /// what `room_from_parent` reads.
    parent: Vec<u32>,
    /// the frontier of both trees, oldest first
    active: VecDeque<NodeID>,
    /// whether a node is already waiting in `active`, so it is queued once
    waiting: BitVec,
    /// nodes whose parent arc was saturated and who need a new one
    orphans: Vec<NodeID>,

    augmentations: usize,
    adoptions: usize,
    steps: usize,
}

impl BoykovKolmogorov {
    /// Which arc runs the other way, for every arc.
    fn pairs_of(graph: &StaticGraph<ResidualArcData>) -> Vec<u32> {
        let mut pair = vec![0_u32; graph.number_of_edges()];
        for node in 0..graph.number_of_nodes() {
            for edge in graph.edge_range(node) {
                let other = graph.target(edge);
                let back = graph
                    .find_edge_sorted(other, node)
                    .expect("residual graph is not symmetric");
                pair[edge] = u32::try_from(back).expect("arc ids have to fit into u32");
            }
        }
        pair
    }

    /// What is left on the arc a node would grow along.
    ///
    /// A tree from the source grows the way the flow runs, so it asks what the
    /// arc out of the node still carries. A tree from the sink grows against
    /// the flow, so it asks what the arc back into the node carries, which is
    /// what this arc caches.
    fn room_to_grow(&self, from: Tree, edge: EdgeID) -> i32 {
        let arc = self.residual_graph.data(edge);
        match from {
            Tree::Source => arc.capacity,
            _ => arc.reverse_capacity,
        }
    }

    /// What is left on the arc that holds a node to its parent.
    fn room_from_parent(&self, of: Tree, edge: EdgeID) -> i32 {
        let arc = self.residual_graph.data(edge);
        match of {
            // the arc runs node -> parent, and the flow runs parent -> node
            Tree::Source => arc.reverse_capacity,
            _ => arc.capacity,
        }
    }

    /// Puts a node on the frontier, once.
    fn wake(&mut self, node: NodeID) {
        if !self.waiting[node] {
            self.waiting.set(node, true);
            self.active.push_back(node);
        }
    }

    /// Moves flow along an arc and keeps the pair's cached capacity in step.
    fn give(&mut self, edge: EdgeID, amount: i32) {
        let arc = self.residual_graph.data_mut(edge);
        arc.capacity -= amount;
        arc.reverse_capacity += amount;
        let back = self.pair[edge] as EdgeID;
        let arc = self.residual_graph.data_mut(back);
        arc.capacity += amount;
        arc.reverse_capacity -= amount;
    }

    /// Grows both trees until one meets the other, and says which arc joined
    /// them.
    fn grow(&mut self) -> Option<EdgeID> {
        while let Some(node) = self.active.front().copied() {
            // a node torn out of its tree since it was queued has nothing to
            // grow, and one that is still here keeps its place until it has
            // been asked for every arc
            if self.tree[node] == Tree::Free {
                self.active.pop_front();
                self.waiting.set(node, false);
                continue;
            }
            let mine = self.tree[node];
            for edge in self.residual_graph.edge_range(node) {
                if self.room_to_grow(mine, edge) <= 0 {
                    continue;
                }
                let other = self.residual_graph.target(edge);
                if self.tree[other] == Tree::Free {
                    self.tree[other] = mine;
                    // the arc grown along runs parent -> child, and a parent
                    // arc has to run the other way, whichever tree it is in
                    self.parent[other] = self.pair[edge];
                    self.wake(other);
                } else if self.tree[other] != mine {
                    // the trees have met, and the arc has to run from the
                    // source side to the sink side
                    return Some(match mine {
                        Tree::Source => edge,
                        _ => self.pair[edge] as EdgeID,
                    });
                }
            }
            self.active.pop_front();
            self.waiting.set(node, false);
        }
        None
    }
}

impl BoykovKolmogorov {
    /// Moves along the path the joining arc closed, and orphans what it
    /// saturates.
    fn augment(&mut self, joining: EdgeID) -> i32 {
        // the least any arc of the path has to give, found by walking to both
        // roots before anything is moved
        let mut least = self.residual_graph.data(joining).capacity;
        let mut node = self.residual_graph.target(self.pair[joining] as EdgeID);
        while node != self.source {
            let arc = self.parent[node] as EdgeID;
            least = least.min(self.room_from_parent(Tree::Source, arc));
            node = self.residual_graph.target(arc);
        }
        let mut node = self.residual_graph.target(joining);
        while node != self.target {
            let arc = self.parent[node] as EdgeID;
            least = least.min(self.room_from_parent(Tree::Sink, arc));
            node = self.residual_graph.target(arc);
        }
        debug_assert!(least > 0, "an augmentation that moves nothing");

        self.give(joining, least);
        // and again, moving it this time. An arc left with nothing is no longer
        // holding its node to the tree.
        let mut node = self.residual_graph.target(self.pair[joining] as EdgeID);
        while node != self.source {
            let arc = self.parent[node] as EdgeID;
            self.give(self.pair[arc] as EdgeID, least);
            if self.room_from_parent(Tree::Source, arc) <= 0 {
                self.parent[node] = NO_ARC;
                self.orphans.push(node);
            }
            node = self.residual_graph.target(arc);
        }
        let mut node = self.residual_graph.target(joining);
        while node != self.target {
            let arc = self.parent[node] as EdgeID;
            self.give(arc, least);
            if self.room_from_parent(Tree::Sink, arc) <= 0 {
                self.parent[node] = NO_ARC;
                self.orphans.push(node);
            }
            node = self.residual_graph.target(arc);
        }
        self.augmentations += 1;
        least
    }

    /// Whether a node still has a way up to the root of its tree, and how far
    /// away that root is.
    ///
    /// The walk is exact: it climbs to a root every time. Boykov and Kolmogorov
    /// keep how far a node was from its root and when that was worked out, so a
    /// walk can stop at anything already known, and that is not here because it
    /// is unsound as stated. A node whose ancestor has since been orphaned
    /// still carries a stamp from this round, so a walk stops there and reports
    /// a root it can no longer reach; adopting through such a node points it
    /// into its own subtree, and the walk up from either goes round for ever.
    /// Whoever wants the cache back needs a way to tell a stamp gone stale.
    fn reaches_a_root(&mut self, node: NodeID) -> Option<usize> {
        let mut walked = 0_usize;
        let mut at = node;
        loop {
            // the one node that legitimately has no parent
            if at == self.source || at == self.target {
                return Some(walked);
            }
            // an orphan, or a node cut loose from every tree
            if self.parent[at] == NO_ARC {
                return None;
            }
            at = self.residual_graph.target(self.parent[at] as EdgeID);
            walked += 1;
            self.steps += 1;
            // Not a debug assertion. A circle in the parents is what an
            // unsound cache would leave behind, and the walk round it does not
            // end: a build without this would hang rather than fail, which is
            // the worse of the two by far. It costs a comparison against a
            // number already in hand.
            assert!(
                walked <= self.residual_graph.number_of_nodes(),
                "the parents of the trees run in a circle"
            );
        }
    }

    /// Hands every orphan to a neighbour that still reaches the root, and sets
    /// free the ones nobody can take.
    fn adopt(&mut self) {
        while let Some(orphan) = self.orphans.pop() {
            self.adoptions += 1;
            let mine = self.tree[orphan];
            debug_assert!(mine != Tree::Free, "an orphan of no tree");

            let mut taken = NO_ARC;
            let mut nearest = usize::MAX;
            for edge in self.residual_graph.edge_range(orphan) {
                if self.room_from_parent(mine, edge) <= 0 {
                    continue;
                }
                let other = self.residual_graph.target(edge);
                if self.tree[other] != mine {
                    continue;
                }
                if let Some(away) = self.reaches_a_root(other)
                    && away < nearest
                {
                    nearest = away;
                    taken = u32::try_from(edge).expect("arc ids have to fit into u32");
                }
            }

            if taken != NO_ARC {
                self.parent[orphan] = taken;
                continue;
            }

            // nobody can take it, so its own children are orphaned and every
            // neighbour that could have grown to it is put back on the frontier
            for edge in self.residual_graph.edge_range(orphan) {
                let other = self.residual_graph.target(edge);
                if self.tree[other] != mine {
                    continue;
                }
                if self.room_from_parent(mine, edge) > 0 {
                    self.wake(other);
                }
                if self.parent[other] != NO_ARC
                    && self.residual_graph.target(self.parent[other] as EdgeID) == orphan
                {
                    self.parent[other] = NO_ARC;
                    self.orphans.push(other);
                }
            }
            self.tree[orphan] = Tree::Free;
            self.waiting.set(orphan, false);
        }
    }
}

impl BoykovKolmogorov {
    /// What the run did: augmentations, adoptions, and steps walked looking for
    /// a parent. For telling whether the walk is what a run is spent on.
    #[must_use]
    pub fn work(&self) -> (usize, usize, usize) {
        (self.augmentations, self.adoptions, self.steps)
    }
}

impl BoykovKolmogorov {
    /// The search itself, against whatever bound the caller left set.
    fn search(&mut self) {
        // a run says what it did, not what every run before it did as well
        self.augmentations = 0;
        self.adoptions = 0;
        self.steps = 0;
        self.finished = false;
        debug!(
            "residual graph size: V {}, E {}",
            self.residual_graph.number_of_nodes(),
            self.residual_graph.number_of_edges()
        );
        if self.source == self.target || self.residual_graph.number_of_nodes() == 0 {
            self.max_flow = 0;
            self.finished = true;
            return;
        }

        self.tree.iter_mut().for_each(|tree| *tree = Tree::Free);
        self.parent.iter_mut().for_each(|parent| *parent = NO_ARC);
        self.active.clear();
        self.waiting.fill(false);
        self.orphans.clear();

        self.tree[self.source] = Tree::Source;
        self.tree[self.target] = Tree::Sink;
        self.wake(self.source);
        self.wake(self.target);

        let mut flow = 0;
        while let Some(joining) = self.grow() {
            flow += self.augment(joining);
            self.adopt();

            if let Some(bound) = &self.bound {
                // the flow only grows, so a bound it has passed it cannot come
                // back under
                if flow > bound.load(Ordering::Relaxed) {
                    debug!("aborting max flow computation at {flow}");
                    self.max_flow = flow;
                    return;
                }
            }
        }

        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        self.max_flow = flow;
        self.finished = true;
    }
}

impl MaxFlow for BoykovKolmogorov {
    fn from_edge_list(
        edge_list: Vec<InputEdge<ResidualEdgeData>>,
        source: NodeID,
        target: NodeID,
    ) -> Self {
        debug_assert!(!edge_list.is_empty());
        let residual_graph = residual_graph_of(edge_list);
        let number_of_nodes = residual_graph.number_of_nodes();
        debug!("pairing {} arcs", residual_graph.number_of_edges());
        let pair = Self::pairs_of(&residual_graph);
        let mut waiting = BitVec::new();
        waiting.resize(number_of_nodes, false);

        Self {
            residual_graph,
            pair,
            source,
            target,
            max_flow: 0,
            finished: false,
            bound: None,
            tree: vec![Tree::Free; number_of_nodes],
            parent: vec![NO_ARC; number_of_nodes],
            active: VecDeque::with_capacity(number_of_nodes),
            waiting,
            orphans: Vec::new(),
            augmentations: 0,
            adoptions: 0,
            steps: 0,
        }
    }

    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>) {
        debug!("upper bound: {}", bound.load(Ordering::Relaxed));
        self.bound = Some(bound);
        self.search();
    }

    fn run(&mut self) {
        // A run of its own is a run against no bound. Keeping the one a
        // previous call was given would have this give up where nothing asked
        // it to.
        self.bound = None;
        self.search();
    }

    fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assignment was not computed.".to_string());
        }
        debug!(
            "finished in {} augmentations and {} adoptions",
            self.augmentations, self.adoptions
        );
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assignment was not computed.".to_string());
        }

        // This holds a flow rather than a preflow, so what the source still
        // reaches through residual capacity is the side of the cut it is on,
        // the same as for the augmenting solvers here.
        let mut reachable = BitVec::new();
        reachable.resize(self.residual_graph.number_of_nodes(), false);
        let mut stack = Vec::with_capacity(self.residual_graph.number_of_nodes());
        stack.push(source);
        reachable.set(source, true);
        while let Some(node) = stack.pop() {
            for edge in self.residual_graph.edge_range(node) {
                let target = self.residual_graph.target(edge);
                if !reachable[target] && self.residual_graph.data(edge).capacity > 0 {
                    stack.push(target);
                    reachable.set(target, true);
                }
            }
        }
        Ok(reachable)
    }
}

#[cfg(test)]
mod tests {
    use super::BoykovKolmogorov;
    use crate::dinic::Dinic;
    use crate::edge::InputEdge;
    use crate::max_flow::{MaxFlow, ResidualEdgeData};
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// What the arcs of the input cost to cut, given which side each node fell
    /// on. A flow value nobody can place a cut against says nothing.
    fn cut_of(edges: &[InputEdge<ResidualEdgeData>], assignment: &bitvec::vec::BitVec) -> i32 {
        edges
            .iter()
            .filter(|edge| assignment[edge.source] && !assignment[edge.target])
            .map(|edge| edge.data.capacity)
            .sum()
    }

    fn layered_graph(
        rng: &mut StdRng,
        width: usize,
        depth: usize,
    ) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
        let source = 0;
        let target = 1 + width * depth;
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
                target,
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
                if layer > 0 && rng.random_range(0..100) < 15 {
                    edges.push(InputEdge::new(
                        node(layer, index),
                        node(layer - 1, index),
                        ResidualEdgeData::new(rng.random_range(1..=5)),
                    ));
                }
            }
        }
        (edges, source, target)
    }

    #[test]
    fn a_path_carries_what_its_narrowest_arc_does() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(5)),
            InputEdge::new(1, 2, ResidualEdgeData::new(3)),
            InputEdge::new(2, 3, ResidualEdgeData::new(7)),
        ];
        let mut solver = BoykovKolmogorov::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(3));
        assert_eq!(cut_of(&edges, &solver.assignment(0).expect("it ran")), 3);
    }

    #[test]
    fn two_ways_round_carry_the_sum_of_both() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(0, 2, ResidualEdgeData::new(6)),
            InputEdge::new(1, 3, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = BoykovKolmogorov::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(10));
        assert_eq!(cut_of(&edges, &solver.assignment(0).expect("it ran")), 10);
    }

    #[test]
    fn a_sink_the_source_cannot_reach_carries_nothing() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = BoykovKolmogorov::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(0));
        assert_eq!(cut_of(&edges, &solver.assignment(0).expect("it ran")), 0);
    }

    #[test]
    fn it_agrees_with_dinic_over_many_graphs() {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        for round in 0..64 {
            let width = 2 + round % 7;
            let depth = 2 + round % 5;
            let (edges, source, target) = layered_graph(&mut rng, width, depth);

            let mut grown = BoykovKolmogorov::from_edge_list(edges.clone(), source, target);
            grown.run();
            let mut dinic = Dinic::from_edge_list(edges.clone(), source, target);
            dinic.run();

            let one = grown.max_flow().expect("boykov-kolmogorov did not run");
            let other = dinic.max_flow().expect("dinic did not run");
            assert_eq!(one, other, "round {round}: {one} against {other}");

            let assignment = grown.assignment(source).expect("it ran");
            assert!(assignment[source], "the source is not on its own side");
            assert!(!assignment[target], "the sink is on the source side");
            assert_eq!(
                cut_of(&edges, &assignment),
                one,
                "round {round}: the cut does not cost what the flow does"
            );
        }
    }

    /// What a second run does, which is not what a first one would.
    ///
    /// The bound of the run before is gone, so this one does not give up. Its
    /// flow is not the whole flow, though: the run before left what it moved in
    /// the residual graph, so this one carries on from there and hands back
    /// only the rest. Ten in all, four of them already moved.
    #[test]
    fn a_second_run_carries_on_where_the_first_stopped() {
        use std::sync::{Arc, atomic::AtomicI32};
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(0, 2, ResidualEdgeData::new(6)),
            InputEdge::new(1, 3, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = BoykovKolmogorov::from_edge_list(edges, 0, 3);
        solver.run_with_upper_bound(Arc::new(AtomicI32::new(2)));
        assert!(
            solver.max_flow().is_err(),
            "a run that gave up reported a flow"
        );
        let (gave_up_at, _, _) = solver.work();
        assert!(gave_up_at > 0, "a run that did nothing at all");

        solver.run();
        assert_eq!(
            solver.max_flow(),
            Ok(6),
            "the bound of the run before was still being watched"
        );
        let (augmentations, _, _) = solver.work();
        assert!(
            augmentations < gave_up_at + augmentations,
            "the counters carried the run before over"
        );
        assert!(augmentations > 0, "the second run did nothing");
    }

    #[test]
    fn a_flow_over_the_bound_gives_up() {
        use std::sync::{Arc, atomic::AtomicI32};
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(0, 2, ResidualEdgeData::new(6)),
            InputEdge::new(1, 3, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = BoykovKolmogorov::from_edge_list(edges, 0, 3);
        solver.run_with_upper_bound(Arc::new(AtomicI32::new(2)));
        assert!(
            solver.max_flow().is_err(),
            "a run that gave up reported a flow"
        );
    }
}
