//! A max-flow solver that repairs its distance labels locally instead of
//! rebuilding them with a full BFS every phase. See issue #545.
//!
//! `level[v]` is the exact residual distance from `v` to the target. The
//! enabling fact is the monotonicity lemma: augmenting only along level
//! respecting paths never decreases a residual distance, because saturating an
//! admissible arc removes forward capacity and adds a back arc from level d+1
//! to level d, which cannot create a shorter path. Distances therefore only
//! rise, the level graph evolves decrementally, and exact labels can be
//! maintained with Even-Shiloach style repair.
//!
//! Two consequences shape the code:
//!
//! * With exact labels, a walk that always steps to a neighbour one level lower
//!   is guaranteed to reach the target, so the augmenting search never
//!   backtracks. Classic Dinic needs a backtracking DFS only because its labels
//!   are a filter rather than exact distances.
//! * Repair needs the residual capacity of the arcs entering an orphan. Every
//!   arc record already caches the capacity of its reverse arc, so that is a
//!   sequential scan of the orphan's own adjacency block with no pointer
//!   chasing.
use crate::{
    edge::InputEdge,
    graph::{EdgeID, Graph, NodeID},
    max_flow::{MaxFlow, ResidualArcData, ResidualEdgeData},
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

/// distance of a node that cannot reach the target any more
const UNREACHABLE: u32 = u32::MAX;

pub struct IncrementalDinic {
    residual_graph: StaticGraph<ResidualArcData>,
    /// exact residual distance to the target
    level: Vec<u32>,
    /// next arc of a node that may still be admissible. Everything before it is
    /// known to be inadmissible and, while the node keeps its level, to stay so.
    current_arc: Vec<u32>,
    /// nodes waiting to be relabelled, bucketed by the level they had when they
    /// were queued, so that repair proceeds in increasing distance order
    buckets: Vec<Vec<NodeID>>,
    queued: BitVec,
    /// the augmenting path currently being walked, as arc ids
    path: Vec<EdgeID>,
    queue: VecDeque<NodeID>,
    max_flow: i32,
    finished: bool,
    relabels: usize,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl IncrementalDinic {
    /// One full sweep, only ever run once, to establish exact distances.
    fn initial_bfs(&mut self) {
        self.level.fill(UNREACHABLE);
        self.level[self.target] = 0;
        self.queue.clear();
        self.queue.push_back(self.target);

        while let Some(u) = self.queue.pop_front() {
            let next = self.level[u] + 1;
            for edge in self.residual_graph.edge_range(u) {
                let v = self.residual_graph.target(edge);
                if self.level[v] != UNREACHABLE {
                    continue;
                }
                // the arc that matters is v -> u, whose capacity this record caches
                if self.residual_graph.data(edge).reverse_capacity < 1 {
                    continue;
                }
                self.level[v] = next;
                self.queue.push_back(v);
            }
        }
    }

    /// True if the arc may be used on a shortest path out of `u`.
    fn is_admissible(&self, u: NodeID, edge: EdgeID) -> bool {
        let data = self.residual_graph.data(edge);
        if data.capacity < 1 {
            return false;
        }
        let v = self.residual_graph.target(edge);
        self.level[u] != UNREACHABLE && self.level[v] + 1 == self.level[u]
    }

    /// Advances the current arc of `u` to the next admissible one and returns
    /// it, or `None` if `u` has none left and has to be relabelled.
    fn advance(&mut self, u: NodeID) -> Option<EdgeID> {
        let end = self.residual_graph.end_edges(u);
        let mut arc = self.current_arc[u] as EdgeID;
        while arc < end {
            if self.is_admissible(u, arc) {
                self.current_arc[u] = arc as u32;
                return Some(arc);
            }
            arc += 1;
        }
        self.current_arc[u] = end as u32;
        None
    }

    /// Walks from the source to the target along admissible arcs. With exact
    /// labels every node on the way has an admissible arc, so this only fails
    /// if the source itself has none left.
    fn find_path(&mut self) -> bool {
        self.path.clear();
        let mut u = self.source;
        while u != self.target {
            let Some(arc) = self.advance(u) else {
                // only the node the walk is standing on can be short of arcs,
                // and by exactness that node has to be the source
                debug_assert_eq!(
                    u, self.source,
                    "an interior node ran out of admissible arcs, so the labels are not exact"
                );
                return false;
            };
            self.path.push(arc);
            u = self.residual_graph.target(arc);
        }
        true
    }

    /// Pushes flow along the path found by [`find_path`] and returns its value.
    /// Saturated arcs leave their tail without a way down, which makes it an
    /// orphan.
    fn augment(&mut self) -> i32 {
        let flow = self
            .path
            .iter()
            .map(|arc| self.residual_graph.data(*arc).capacity)
            .min()
            .expect("augmenting on an empty path");
        debug_assert!(flow > 0);

        let mut tail = self.source;
        for index in 0..self.path.len() {
            let arc = self.path[index];
            let head = self.residual_graph.target(arc);

            let forward = self.residual_graph.data_mut(arc);
            forward.capacity -= flow;
            forward.reverse_capacity += flow;
            let saturated = forward.capacity == 0;

            let partner = self
                .residual_graph
                .find_edge_sorted(head, tail)
                .expect("residual graph is not symmetric");
            let backward = self.residual_graph.data_mut(partner);
            backward.capacity += flow;
            backward.reverse_capacity -= flow;

            if saturated && self.advance(tail).is_none() {
                self.enqueue(tail);
            }
            tail = head;
        }
        flow
    }

    fn enqueue(&mut self, node: NodeID) {
        if node == self.target || self.queued[node] {
            return;
        }
        let level = self.level[node];
        if level == UNREACHABLE {
            return;
        }
        let level = level as usize;
        if self.buckets.len() <= level {
            self.buckets.resize(level + 1, Vec::new());
        }
        self.buckets[level].push(node);
        self.queued.set(node, true);
    }

    /// Restores exact labels after an augmentation.
    ///
    /// Orphans are processed in increasing order of their old level, so a node
    /// is relabelled only after everything it could depend on is already
    /// correct. A node whose recomputed level is unchanged ends the cascade.
    fn repair(&mut self) {
        let horizon = self.residual_graph.number_of_nodes() as u32;
        let mut level = 0;
        while level < self.buckets.len() {
            while let Some(u) = self.buckets[level].pop() {
                self.queued.set(u, false);
                let old = self.level[u];
                if old as usize != level {
                    // already relabelled through a shorter route, it will be
                    // picked up again from its new bucket
                    continue;
                }

                let mut best = UNREACHABLE;
                for arc in self.residual_graph.edge_range(u) {
                    if self.residual_graph.data(arc).capacity < 1 {
                        continue;
                    }
                    let neighbour = self.residual_graph.target(arc);
                    // a self loop offers no way towards the target, and taking
                    // it as support would raise the level by one forever
                    if neighbour == u {
                        continue;
                    }
                    let candidate = self.level[neighbour];
                    if candidate < best {
                        best = candidate;
                    }
                }
                // A residual distance can never exceed n - 1, so anything past
                // that means the target is out of reach. Without this cap two
                // nodes whose only remaining arcs point at each other keep
                // raising one another by one for ever.
                let new = if best == UNREACHABLE || best + 1 >= horizon {
                    UNREACHABLE
                } else {
                    best + 1
                };
                if new == old {
                    continue;
                }
                debug_assert!(new > old, "a distance decreased, which breaks monotonicity");
                self.level[u] = new;
                self.relabels += 1;
                // the node may use a different arc now, and the arcs it skipped
                // were skipped against its old level
                self.current_arc[u] = self.residual_graph.begin_edges(u) as u32;

                // whoever reached the target through u at the old distance has
                // to be checked. The record in u's own block caches the capacity
                // of the arc coming in, so this is one sequential scan.
                for arc in self.residual_graph.edge_range(u) {
                    if self.residual_graph.data(arc).reverse_capacity < 1 {
                        continue;
                    }
                    let neighbour = self.residual_graph.target(arc);
                    if neighbour != u && self.level[neighbour] == old + 1 {
                        self.enqueue(neighbour);
                    }
                }
            }
            level += 1;
        }
        self.buckets.clear();
    }

    /// Recomputes the labels from scratch and compares. Far too slow to ship,
    /// exactly right for the property tests, and the fastest way to find the
    /// operation that broke an invariant rather than the run that failed.
    #[cfg(debug_assertions)]
    fn assert_levels_exact(&mut self, when: &str) {
        let maintained = self.level.clone();
        self.initial_bfs();
        for (node, kept) in maintained.iter().enumerate() {
            assert_eq!(
                *kept, self.level[node],
                "{when}: level of node {node} is {kept} but the exact distance is {}",
                self.level[node]
            );
        }
    }
}

impl MaxFlow for IncrementalDinic {
    fn from_edge_list(
        edge_list: Vec<InputEdge<ResidualEdgeData>>,
        source: usize,
        target: usize,
    ) -> Self {
        let residual_graph = crate::residual_graph::build_residual_graph(edge_list);
        let number_of_nodes = residual_graph.number_of_nodes();
        Self {
            residual_graph,
            level: Vec::new(),
            current_arc: Vec::new(),
            buckets: Vec::new(),
            queued: BitVec::new(),
            path: Vec::new(),
            queue: VecDeque::with_capacity(number_of_nodes),
            max_flow: 0,
            finished: false,
            relabels: 0,
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
        self.level.resize(number_of_nodes, UNREACHABLE);
        self.current_arc.resize(number_of_nodes, 0);
        self.queued.resize(number_of_nodes, false);
        for node in 0..number_of_nodes {
            self.current_arc[node] = self.residual_graph.begin_edges(node) as u32;
        }

        self.initial_bfs();
        #[cfg(debug_assertions)]
        self.assert_levels_exact("after the initial sweep");

        let mut flow = 0;
        while self.level[self.source] != UNREACHABLE {
            if !self.find_path() {
                // the source has no admissible arc, so its own label is stale
                self.enqueue(self.source);
                self.repair();
                #[cfg(debug_assertions)]
                self.assert_levels_exact("after relabelling the source");
                continue;
            }
            flow += self.augment();
            self.repair();
            #[cfg(debug_assertions)]
            self.assert_levels_exact("after an augmentation");

            if let Some(bound) = &self.bound
                && flow > bound.load(Ordering::Relaxed)
            {
                debug!("aborting max flow computation at {flow}");
                self.max_flow = flow;
                return;
            }
        }

        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        debug!("{} relabels", self.relabels);
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
