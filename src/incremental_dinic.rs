//! A max-flow solver that relabels lazily instead of rebuilding the level graph
//! with a full BFS every phase. See issue #545.
//!
//! The labels are not distances. They are a valid labelling in the push-relabel
//! sense: `level[target] == 0`, and `level[u] <= level[v] + 1` for every arc
//! `u -> v` that still has residual capacity. Any such labelling is a lower
//! bound on the true residual distance, which is all that is needed:
//!
//! * an arc may be walked only if it strictly lowers the label, so a walk cannot
//!   cycle and has to end at the target
//! * once `level[source]` reaches the node count no augmenting path can exist,
//!   because a path has at most `n - 1` arcs and each one lowers the label by at
//!   least one
//!
//! Dropping exactness is what makes the repair cheap. Raising a label can only
//! give the arcs coming into that node more slack, never less, so a node that
//! runs out of arcs is relabelled on its own with one scan of its adjacency and
//! nothing cascades. An earlier version of this file maintained exact distances
//! and had to propagate every change, which let mutually supporting nodes raise
//! each other one level at a time and made it thousands of times slower than the
//! solver it was meant to replace.
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

pub struct IncrementalDinic {
    residual_graph: StaticGraph<ResidualArcData>,
    /// a valid labelling, not a distance. Reaching `horizon` means the target is
    /// out of reach.
    level: Vec<u32>,
    /// next arc of a node that may still be admissible. Everything before it is
    /// inadmissible and stays so until the node is relabelled.
    current_arc: Vec<u32>,
    /// how many nodes carry each label, so that an empty label can be spotted
    count: Vec<u32>,
    /// arcs of the walk currently being extended, from the source outwards
    path: Vec<EdgeID>,
    queue: VecDeque<NodeID>,
    horizon: u32,
    max_flow: i32,
    finished: bool,
    relabels: usize,
    gaps: usize,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl IncrementalDinic {
    /// One full sweep, run once, to start from the tightest valid labelling
    /// there is.
    fn initial_bfs(&mut self) {
        self.level.fill(self.horizon);
        self.level[self.target] = 0;
        self.queue.clear();
        self.queue.push_back(self.target);

        while let Some(u) = self.queue.pop_front() {
            let next = self.level[u] + 1;
            if next >= self.horizon {
                continue;
            }
            for edge in self.residual_graph.edge_range(u) {
                let v = self.residual_graph.target(edge);
                if self.level[v] != self.horizon {
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

    /// Advances the current arc of `u` to the next one that has capacity and
    /// leads to a strictly lower label.
    fn advance(&mut self, u: NodeID) -> Option<EdgeID> {
        let end = self.residual_graph.end_edges(u);
        let mut arc = self.current_arc[u] as EdgeID;
        let level = self.level[u];
        while arc < end {
            let entry = self.residual_graph.data(arc);
            if entry.capacity > 0 && self.level[self.residual_graph.target(arc)] < level {
                self.current_arc[u] = arc as u32;
                return Some(arc);
            }
            arc += 1;
        }
        self.current_arc[u] = end as u32;
        None
    }

    /// Raises the label of a node that has no admissible arc left, to one more
    /// than the lowest label it can still reach. That restores an admissible arc
    /// unless the node can reach nothing at all, and it cannot invalidate the
    /// labelling anywhere else, because arcs into `u` only gain slack when
    /// `level[u]` grows.
    fn relabel(&mut self, u: NodeID) {
        let mut best = self.horizon;
        for arc in self.residual_graph.edge_range(u) {
            if self.residual_graph.data(arc).capacity < 1 {
                continue;
            }
            let neighbour = self.residual_graph.target(arc);
            // a self loop leads nowhere and would raise the label for ever
            if neighbour == u {
                continue;
            }
            let candidate = self.level[neighbour];
            if candidate < best {
                best = candidate;
            }
        }
        let raised = best.saturating_add(1).min(self.horizon);
        let old = self.level[u];
        debug_assert!(raised > old, "a label must never fall");
        self.count[old as usize] -= 1;
        if raised < self.horizon {
            self.count[raised as usize] += 1;
        }
        self.level[u] = raised;
        self.current_arc[u] = self.residual_graph.begin_edges(u) as u32;
        self.relabels += 1;

        // If no node carries the label `old` any more, then nothing above it can
        // reach the target: a residual path drops the label by at most one per
        // arc, so it would have to pass through a node labelled `old`. Those
        // nodes can go straight to the horizon instead of climbing there one
        // step at a time, which is the difference between this terminating in
        // milliseconds and in minutes.
        if old > 0 && old < self.horizon && self.count[old as usize] == 0 {
            self.apply_gap(old);
        }
    }

    /// Retires every node above an empty label.
    fn apply_gap(&mut self, gap: u32) {
        self.gaps += 1;
        for node in 0..self.level.len() {
            let level = self.level[node];
            if level > gap && level < self.horizon {
                self.count[level as usize] -= 1;
                self.level[node] = self.horizon;
                self.current_arc[node] = self.residual_graph.begin_edges(node) as u32;
            }
        }
        // the walk may have been standing on a node that has just retired
        self.path.clear();
    }

    /// Pushes flow along the walked path and returns its value.
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

            let partner = self
                .residual_graph
                .find_edge_sorted(head, tail)
                .expect("residual graph is not symmetric");
            let backward = self.residual_graph.data_mut(partner);
            backward.capacity += flow;
            backward.reverse_capacity -= flow;

            tail = head;
        }
        flow
    }

    /// The node the walk currently stands on.
    fn head_of_walk(&self) -> NodeID {
        match self.path.last() {
            Some(arc) => self.residual_graph.target(*arc),
            None => self.source,
        }
    }

    /// Cuts the walk back to the tail of the arc closest to the source that the
    /// last augmentation saturated. Everything before that is still usable.
    fn retreat_past_saturated(&mut self) {
        let first_saturated = self
            .path
            .iter()
            .position(|arc| self.residual_graph.data(*arc).capacity == 0);
        match first_saturated {
            Some(index) => self.path.truncate(index),
            None => self.path.clear(),
        }
    }

    /// Checks the labelling rather than the distances, since exactness is no
    /// longer the contract. Too slow to ship, right for the property tests.
    #[cfg(debug_assertions)]
    fn assert_valid_labelling(&self, when: &str) {
        assert_eq!(self.level[self.target], 0, "{when}: the target label moved");
        for u in self.residual_graph.node_range() {
            for arc in self.residual_graph.edge_range(u) {
                if self.residual_graph.data(arc).capacity < 1 {
                    continue;
                }
                let v = self.residual_graph.target(arc);
                assert!(
                    self.level[u] <= self.level[v].saturating_add(1),
                    "{when}: arc {u} -> {v} has residual capacity but its labels are \
                     {} and {}, which breaks the labelling",
                    self.level[u],
                    self.level[v]
                );
            }
        }
    }

    /// At the end there must be no residual path from source to target, which is
    /// the property the flow value rests on.
    #[cfg(debug_assertions)]
    fn assert_no_residual_path(&self) {
        let mut seen: BitVec = BitVec::repeat(false, self.residual_graph.number_of_nodes());
        let mut stack = vec![self.source];
        seen.set(self.source, true);
        while let Some(u) = stack.pop() {
            for arc in self.residual_graph.edge_range(u) {
                if self.residual_graph.data(arc).capacity < 1 {
                    continue;
                }
                let v = self.residual_graph.target(arc);
                assert_ne!(v, self.target, "an augmenting path was left behind");
                if !seen[v] {
                    seen.set(v, true);
                    stack.push(v);
                }
            }
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
            count: Vec::new(),
            path: Vec::new(),
            queue: VecDeque::with_capacity(number_of_nodes),
            horizon: number_of_nodes as u32,
            max_flow: 0,
            finished: false,
            relabels: 0,
            gaps: 0,
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
        self.level.resize(number_of_nodes, self.horizon);
        self.current_arc.resize(number_of_nodes, 0);
        for node in 0..number_of_nodes {
            self.current_arc[node] = self.residual_graph.begin_edges(node) as u32;
        }

        self.initial_bfs();
        self.count.clear();
        self.count.resize(number_of_nodes + 1, 0);
        for node in 0..number_of_nodes {
            let level = self.level[node];
            if level < self.horizon {
                self.count[level as usize] += 1;
            }
        }
        #[cfg(debug_assertions)]
        self.assert_valid_labelling("after the initial sweep");

        let mut flow = 0;
        self.path.clear();
        while self.level[self.source] < self.horizon {
            let u = self.head_of_walk();
            if u == self.target {
                flow += self.augment();
                self.retreat_past_saturated();

                if let Some(bound) = &self.bound
                    && flow > bound.load(Ordering::Relaxed)
                {
                    debug!("aborting max flow computation at {flow}");
                    self.max_flow = flow;
                    return;
                }
                continue;
            }
            match self.advance(u) {
                Some(arc) => self.path.push(arc),
                None => {
                    // the walk is stuck, so this node's label was too low. It is
                    // raised on its own and the step that led here is undone, so
                    // that the predecessor picks another arc.
                    self.relabel(u);
                    self.path.pop();
                }
            }
        }

        #[cfg(debug_assertions)]
        {
            self.assert_valid_labelling("at the end");
            self.assert_no_residual_path();
        }

        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        debug!("{} relabels, {} gaps", self.relabels, self.gaps);
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
