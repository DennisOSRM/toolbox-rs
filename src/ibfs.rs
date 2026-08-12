//! A max-flow solver in the style of IBFS, Goldberg, Hed, Kaplan, Tarjan and
//! Werneck, ESA 2011. See issue #545.
//!
//! A search tree is grown breadth first from the source and is *maintained*
//! across augmentations rather than rebuilt. What matters is the handling of a
//! tree arc that an augmentation saturates, which orphans the node below it:
//!
//! * the orphan looks for the shallowest tree node that can still feed it and
//!   re-parents onto it, moving deeper if it has to
//! * failing that it leaves the tree, and its own children are orphaned in turn
//!
//! Leaving the tree is the point. Two earlier attempts on this issue kept
//! distance labels and no tree, so a node that had lost its route could only say
//! so by raising its label one step at a time until it reached the horizon. On a
//! road network that cost hundreds of relabels per node. A tree says it in one
//! step, and a node that leaves is picked up again at its proper depth when the
//! tree next grows past it.
//!
//! The layout of the residual graph does the rest. Every arc record carries the
//! capacity of the arc in both directions, so looking for a parent, finding
//! children and growing the frontier are sequential scans of the node's own
//! adjacency block, with no pointer chasing.
use crate::{
    edge::InputEdge,
    graph::{EdgeID, Graph, NodeID},
    max_flow::{MaxFlow, ResidualArcData, ResidualEdgeData},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use log::debug;
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

/// a node that belongs to no tree
const FREE: u32 = u32::MAX;
/// no parent: the root, and orphans being processed
const NO_ARC: u32 = u32::MAX;

pub struct Ibfs {
    residual_graph: StaticGraph<ResidualArcData>,
    /// depth in the tree, or `FREE`
    label: Vec<u32>,
    /// the arc **in this node's own block** that points at its parent, so the
    /// capacity of the arc coming from the parent is already in the record
    parent_arc: Vec<u32>,
    /// nodes waiting to be expanded, bucketed by depth, so a node re-parented
    /// deeper is expanded later instead of being lost
    frontiers: Vec<Vec<NodeID>>,
    level: usize,
    orphans: Vec<NodeID>,
    max_flow: i32,
    finished: bool,
    adoptions: usize,
    expulsions: usize,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl Ibfs {
    fn parent_of(&self, u: NodeID) -> Option<NodeID> {
        match self.parent_arc[u] {
            NO_ARC => None,
            arc => Some(self.residual_graph.target(arc as EdgeID)),
        }
    }

    /// Capacity of the arc running from the parent of `u` into `u`, taken from
    /// the cached reverse capacity of `u`'s own record.
    fn capacity_from_parent(&self, u: NodeID) -> i32 {
        match self.parent_arc[u] {
            NO_ARC => 0,
            arc => self.residual_graph.data(arc as EdgeID).reverse_capacity,
        }
    }

    /// True if the parent chain of `u` still ends at the source. An orphan must
    /// not adopt a node out of its own subtree, and walking the chain is the
    /// cheapest way to tell.
    fn reaches_source(&self, mut u: NodeID) -> bool {
        for _ in 0..=self.residual_graph.number_of_nodes() {
            if u == self.source {
                return true;
            }
            if self.label[u] == FREE {
                return false;
            }
            match self.parent_of(u) {
                Some(parent) => u = parent,
                None => return false,
            }
        }
        false
    }

    /// The shallowest tree node that can still feed `u`, as the arc in `u`'s own
    /// block that points at it.
    fn find_parent(&self, u: NodeID) -> Option<EdgeID> {
        let mut best: Option<(u32, EdgeID)> = None;
        for arc in self.residual_graph.edge_range(u) {
            // the arc that would carry the flow is neighbour -> u
            if self.residual_graph.data(arc).reverse_capacity < 1 {
                continue;
            }
            let neighbour = self.residual_graph.target(arc);
            if neighbour == u {
                continue;
            }
            let label = self.label[neighbour];
            if label == FREE || best.is_some_and(|(seen, _)| label >= seen) {
                continue;
            }
            if self.reaches_source(neighbour) {
                best = Some((label, arc));
                if label + 1 == self.label[u] {
                    // nothing shallower can exist, so stop looking
                    break;
                }
            }
        }
        best.map(|(_, arc)| arc)
    }

    /// Re-parents the orphans, moving them deeper when that is the only option
    /// and dropping them out of the tree when nothing can feed them.
    fn adopt(&mut self) {
        while let Some(u) = self.orphans.pop() {
            if u == self.source || self.label[u] == FREE {
                continue;
            }
            let old = self.label[u];
            match self.find_parent(u) {
                Some(arc) => {
                    let parent = self.residual_graph.target(arc);
                    let depth = self.label[parent] + 1;
                    self.parent_arc[u] = arc as u32;
                    self.adoptions += 1;
                    if depth != old {
                        self.orphan_children(u, old);
                        self.label[u] = depth;
                        self.enqueue(u);
                    }
                }
                None => {
                    self.orphan_children(u, old);
                    self.label[u] = FREE;
                    self.parent_arc[u] = NO_ARC;
                    self.expulsions += 1;
                }
            }
        }
    }

    /// Orphans every child of `u`, which is any tree neighbour naming `u` as its
    /// parent. One scan of `u`'s own adjacency block.
    fn orphan_children(&mut self, u: NodeID, old_label: u32) {
        for arc in self.residual_graph.edge_range(u) {
            let child = self.residual_graph.target(arc);
            if child != u && self.label[child] == old_label + 1 && self.parent_of(child) == Some(u)
            {
                self.parent_arc[child] = NO_ARC;
                self.orphans.push(child);
            }
        }
    }

    /// Queues a node to have its neighbours examined, at its current depth.
    fn enqueue(&mut self, u: NodeID) {
        let level = self.label[u] as usize;
        if self.frontiers.len() <= level {
            self.frontiers.resize(level + 1, Vec::new());
        }
        self.frontiers[level].push(u);
    }

    /// Moves `flow` along the arc from `tail` to `head`, keeping the cached
    /// reverse capacities of both records in step.
    fn push(&mut self, tail: NodeID, head: NodeID, arc: EdgeID, flow: i32) {
        let forward = self.residual_graph.data_mut(arc);
        forward.capacity -= flow;
        forward.reverse_capacity += flow;
        let partner = self
            .residual_graph
            .find_edge_sorted(head, tail)
            .expect("residual graph is not symmetric");
        let backward = self.residual_graph.data_mut(partner);
        backward.capacity += flow;
        backward.reverse_capacity -= flow;

        // `head` has just gained outgoing capacity, so a node that could not be
        // reached through it before may be reachable now. If it is in the tree
        // it gets another turn, otherwise a node expelled earlier would never be
        // picked up again and flow would be left on the table.
        if self.label[head] != FREE {
            self.enqueue(head);
        }
    }

    /// Pushes flow from the source down the tree to `u` and over `closing_arc`
    /// into the target. Each arc that saturates orphans the node below it.
    fn augment(&mut self, u: NodeID, closing_arc: EdgeID) -> i32 {
        let mut flow = self.residual_graph.data(closing_arc).capacity;
        let mut walker = u;
        while self.parent_arc[walker] != NO_ARC {
            flow = flow.min(self.capacity_from_parent(walker));
            walker = self.parent_of(walker).expect("checked by the condition");
        }
        debug_assert!(flow > 0, "augmenting on a saturated path");

        // the closing arc belongs to no tree, so it orphans nobody
        self.push(u, self.target, closing_arc, flow);

        let mut walker = u;
        while let Some(parent) = self.parent_of(walker) {
            let record = self.parent_arc[walker] as EdgeID;
            let carrying = self
                .residual_graph
                .find_edge_sorted(parent, walker)
                .expect("residual graph is not symmetric");
            self.push(parent, walker, carrying, flow);
            if self.residual_graph.data(record).reverse_capacity == 0 {
                self.parent_arc[walker] = NO_ARC;
                self.orphans.push(walker);
            }
            walker = parent;
        }
        flow
    }

    /// Expands the nodes queued at the current depth, augmenting whenever the
    /// target is reached.
    fn expand_level(&mut self) -> i32 {
        let mut pushed = 0;
        let frontier = std::mem::take(&mut self.frontiers[self.level]);

        for &u in &frontier {
            let mut arc = self.residual_graph.begin_edges(u);
            let end = self.residual_graph.end_edges(u);
            while arc < end {
                // an augmentation may have moved u out of this level, or out of
                // the tree altogether
                if self.label[u] as usize != self.level {
                    break;
                }
                if self.residual_graph.data(arc).capacity < 1 {
                    arc += 1;
                    continue;
                }
                let v = self.residual_graph.target(arc);
                if v == self.target {
                    pushed += self.augment(u, arc);
                    self.adopt();
                    // capacities and the tree have both moved, so this arc is
                    // examined again rather than stepped over
                    continue;
                }
                if v != u && self.label[v] == FREE {
                    let into_v = self
                        .residual_graph
                        .find_edge_sorted(v, u)
                        .expect("residual graph is not symmetric");
                    self.label[v] = self.label[u] + 1;
                    self.parent_arc[v] = into_v as u32;
                    self.enqueue(v);
                }
                arc += 1;
            }
        }
        pushed
    }
}

impl MaxFlow for Ibfs {
    fn from_edge_list(
        edge_list: Vec<InputEdge<ResidualEdgeData>>,
        source: usize,
        target: usize,
    ) -> Self {
        let residual_graph = crate::residual_graph::build_residual_graph(edge_list);
        Self {
            residual_graph,
            label: Vec::new(),
            parent_arc: Vec::new(),
            frontiers: Vec::new(),
            level: 0,
            orphans: Vec::new(),
            max_flow: 0,
            finished: false,
            adoptions: 0,
            expulsions: 0,
            source,
            target,
            bound: None,
        }
    }

    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>) {
        self.bound = Some(bound);
        self.run()
    }

    fn run(&mut self) {
        let number_of_nodes = self.residual_graph.number_of_nodes();
        self.label.clear();
        self.label.resize(number_of_nodes, FREE);
        self.parent_arc.clear();
        self.parent_arc.resize(number_of_nodes, NO_ARC);
        self.frontiers.clear();
        self.level = 0;

        self.label[self.source] = 0;
        self.enqueue(self.source);

        let mut flow = 0;
        while self.level < self.frontiers.len() {
            flow += self.expand_level();
            if let Some(bound) = &self.bound
                && flow > bound.load(Ordering::Relaxed)
            {
                debug!("aborting max flow computation at {flow}");
                self.max_flow = flow;
                return;
            }
            // a node re-parented onto this level during the sweep is expanded
            // before the search moves on
            if self.frontiers[self.level].is_empty() {
                self.level += 1;
            }
        }

        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        debug!(
            "{} adoptions, {} expulsions",
            self.adoptions, self.expulsions
        );
        self.max_flow = flow;
        self.finished = true;
    }

    fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assignment was not computed.".to_string());
        }
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assignment was not computed.".to_string());
        }
        let mut reachable: BitVec = BitVec::repeat(false, self.residual_graph.number_of_nodes());
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
