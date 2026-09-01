//! A max-flow computation by push-relabel, for the min cut of an inertial flow
//! run.
//!
//! The algorithm is Goldberg and Tarjan's, with the heuristics that make the
//! difference between a textbook version and a usable one:
//!
//! 1) *Lowest label selection.* The active node discharged next is one of the
//!    least height there is. Highest label is the usual choice and has the
//!    better bound, but a height is the distance to the sink, so it discharges
//!    what is furthest from the sink first and delivers nothing there until the
//!    end. That makes an upper bound useless, which is what inertial flow
//!    prunes an axis by. See [`PushRelabel::by_lowest_label`].
//! 2) *Partial augmentation.* A discharge walks a path of admissible arcs and
//!    moves the flow along the whole of it, rather than pushing across one arc
//!    and asking the buckets for the excess again a step further on.
//! 3) *Global relabelling.* Every so often the heights are thrown away and
//!    worked out again by a backward breadth-first search from the sink, which
//!    is the exact distance to it. Heights that are exact rather than merely
//!    valid are what stops the search climbing a node one step at a time. A
//!    graph small enough not to earn that back starts at zero instead.
//! 4) *The gap heuristic.* If no node is left at some height below `n`, then no
//!    node above that height can reach the sink at all, and all of them are
//!    lifted out of the way at once.
//!
//! Only the first phase is run. The second, which returns the excess that never
//! reached the sink and turns the preflow into a flow, costs as much again and
//! says nothing about where the cut is: once no active node can reach the sink,
//! the nodes the source still reaches through residual capacity are one side of
//! a minimum cut, and the flow value is what has arrived at the sink. That is
//! all [`crate::inertial_flow`] asks for.
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

/// How much work is done between two sweeps of the heights, as a multiple of
/// the arcs of the residual graph.
///
/// Cherkassky and Goldberg count the arcs a run has looked at rather than the
/// relabels it has done, since that is what a sweep is being weighed against.
/// Six is the value their measurements settle on and it holds up here.
///
/// The tally costs an increment in the innermost loop, which looks like
/// something to remove until it is removed: counting relabels instead reaches
/// the threshold far later, since work accrues from every arc scanned and not
/// only from a relabel, and the sweeps that stop the heights drifting then come
/// too seldom. A threshold of one relabel a node cost five per cent, two cost
/// eleven, and four cost a third of the run.
const WORK_PER_SWEEP: usize = 6;

/// What a relabel is charged, over the arcs it scans, for the sweep it makes of
/// a node's whole block.
const WORK_OF_A_RELABEL: usize = 12;

/// The end of a bucket's chain of active nodes.
const NOWHERE: u32 = u32::MAX;

/// How many arcs a partial augmentation may walk before it moves what it found.
const PATH_ARCS: usize = 4;

/// How many nodes a graph needs before its heights are worth a sweep to start.
///
/// Exact heights cost a search of the whole cell. A cell settled in a few dozen
/// relabels never earns that back, and a partition of a continent is mostly
/// such cells. Zero is a valid height for everything but the source, so the run
/// simply starts there instead.
const SMALLEST_FOR_A_SWEEP: usize = 1024;

pub struct PushRelabel {
    residual_graph: StaticGraph<ResidualArcData>,
    /// The arc that runs the other way, for each arc.
    ///
    /// A push moves capacity between an arc and its pair, and push-relabel
    /// pushes far more often than an augmenting-path method does. Searching the
    /// other node's block for it every time, as the augmenting methods here do,
    /// is a binary search per push; this is worked out once instead.
    pair: Vec<u32>,
    source: NodeID,
    target: NodeID,
    /// The height that means "cannot reach the sink". A node this high is on
    /// the source side of the cut and has nothing left to do in phase one.
    unreachable: u32,
    max_flow: i32,
    finished: bool,
    bound: Option<Arc<AtomicI32>>,

    /// the distance to the sink, as far as it is known
    height: Vec<u32>,
    /// what has arrived at a node and not left it again
    excess: Vec<i64>,
    /// where the scan of a node's arcs has got to, so that a discharge that
    /// gives up and comes back does not start from the beginning
    current: Vec<EdgeID>,
    /// how many nodes sit at each height, which is what the gap heuristic reads
    at_height: Vec<u32>,
    /// The first active node of each height, and the one after each node.
    ///
    /// A vector per height is the obvious way to bucket and the wrong one here:
    /// the first cut of a continent runs on eighteen million nodes, and a
    /// vector apiece is four hundred megabytes of empty vectors before a single
    /// node is pushed, times the axes that run at once. The buckets are singly
    /// linked through the nodes instead, which is two flat arrays and no
    /// allocation at all. A node is linked at most once: it is only linked when
    /// its excess rises from nothing, and it is unlinked before it is
    /// discharged.
    bucket_head: Vec<u32>,
    bucket_next: Vec<u32>,
    /// the greatest height any bucket holds anything at
    highest: usize,
    /// the least height any bucket holds anything at
    lowest: usize,
    /// Whether to discharge the node nearest the sink rather than the one
    /// furthest from it.
    ///
    /// Highest label is the variant with the better bound and the better run
    /// time to a finished flow. It is also the worst possible order for a run
    /// that may be given up on: a height is the distance to the sink, so it
    /// discharges what is furthest from the sink first and nothing arrives
    /// there until the end. Lowest label delivers to the sink from the start,
    /// which is what a caller watching an upper bound is waiting for.
    lowest_label: bool,
    queue: VecDeque<NodeID>,
    relabels: usize,
    global_relabels: usize,
    pushes: usize,
    /// arcs looked at since the last sweep of the heights
    work: usize,
    /// the arcs of the path being walked, kept so it is not allocated per walk
    path: Vec<EdgeID>,
}

impl PushRelabel {
    /// Discharges the node nearest the sink rather than the one furthest from
    /// it. See the field of the same name for why that is worth having.
    pub fn by_lowest_label(&mut self, yes: bool) {
        self.lowest_label = yes;
    }

    /// What the run did: pushes, relabels, and global relabels. For working
    /// out which of the three a change of heuristic actually moved.
    #[must_use]
    pub fn work(&self) -> (usize, usize, usize) {
        (self.pushes, self.relabels, self.global_relabels)
    }

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

    /// Puts a node at the head of the bucket of its height.
    fn link(&mut self, node: NodeID, height: usize) {
        debug_assert!(self.bucket_next[node] == NOWHERE, "a node linked twice");
        debug_assert!(height < self.bucket_head.len(), "a height past the buckets");
        self.bucket_next[node] = self.bucket_head[height];
        self.bucket_head[height] = u32::try_from(node).expect("the graph is too large to hold");
        self.highest = self.highest.max(height);
        self.lowest = self.lowest.min(height);
    }

    /// Lifts a node to one above the lowest node it can still reach.
    fn relabel(&mut self, node: NodeID) {
        let unreachable = self.unreachable;
        let was = self.height[node];

        let mut lowest = u32::MAX;
        self.work += WORK_OF_A_RELABEL + self.residual_graph.end_edges(node)
            - self.residual_graph.begin_edges(node);
        for edge in self.residual_graph.edge_range(node) {
            if self.residual_graph.data(edge).capacity > 0 {
                lowest = lowest.min(self.height[self.residual_graph.target(edge)]);
            }
        }
        let now = if lowest == u32::MAX {
            unreachable
        } else {
            lowest.saturating_add(1).min(unreachable)
        };

        self.height[node] = now;
        self.relabels += 1;
        self.at_height[was as usize] -= 1;
        self.at_height[now as usize] += 1;

        // The gap: nothing is left at the height this node came from, so
        // nothing above it and below `unreachable` has a way down to the sink
        // either, and every one of them can be lifted out of the way at once.
        if was < unreachable && self.at_height[was as usize] == 0 {
            self.gap(was);
        }
    }

    /// Lifts everything above an empty height out of phase one.
    fn gap(&mut self, empty: u32) {
        let unreachable = self.unreachable;
        for node in 0..self.residual_graph.number_of_nodes() {
            let height = self.height[node];
            if node != self.source && height > empty && height < unreachable {
                self.at_height[height as usize] -= 1;
                self.height[node] = unreachable;
            }
        }
        // the buckets above the gap hold nothing that is still worth taking,
        // and what is left in them is stepped over when it is popped
        self.highest = self.highest.min(empty as usize);
    }

    /// Works the heights out again as the exact distance to the sink.
    fn global_relabel(&mut self) {
        let unreachable = self.unreachable;
        self.global_relabels += 1;
        self.height
            .iter_mut()
            .for_each(|height| *height = unreachable);

        self.height[self.target] = 0;
        self.queue.clear();
        self.queue.push_back(self.target);
        while let Some(node) = self.queue.pop_front() {
            let next = self.height[node] + 1;
            for edge in self.residual_graph.edge_range(node) {
                let other = self.residual_graph.target(edge);
                // An arc that runs into `node` and has capacity left is a step
                // of the backward search. The capacity of that arc is what this
                // one caches, so the pair does not have to be looked up.
                if self.height[other] == unreachable
                    && other != self.source
                    && self.residual_graph.data(edge).reverse_capacity > 0
                {
                    self.height[other] = next;
                    self.queue.push_back(other);
                }
            }
        }
        self.height[self.source] = unreachable;
        self.work = 0;
        self.seed();
    }

    /// Fills the buckets, the counts and the arc cursors from the heights as
    /// they stand, whether those came from a sweep or from nothing at all.
    fn seed(&mut self) {
        // Only the heads. A stale `next` is never walked into, since `link`
        // overwrites it before the node is ever at the head of a chain; the
        // clear below is what the assertion in `link` reads, and a sweep of the
        // nodes is not worth paying for in a build that does not check it.
        self.bucket_head.iter_mut().for_each(|head| *head = NOWHERE);
        if cfg!(debug_assertions) {
            self.bucket_next.iter_mut().for_each(|next| *next = NOWHERE);
        }
        self.at_height.iter_mut().for_each(|count| *count = 0);
        self.highest = 0;
        self.lowest = self.bucket_head.len() - 1;
        // Counting the heights, filling the buckets and winding the arc cursors
        // back are three walks of the same nodes, and this is a quarter of the
        // time the whole run takes, so they are one walk.
        for node in 0..self.residual_graph.number_of_nodes() {
            self.current[node] = self.residual_graph.begin_edges(node);
            let height = self.height[node] as usize;
            if node != self.source {
                self.at_height[height] += 1;
            }
            if node == self.source || node == self.target || self.excess[node] <= 0 {
                continue;
            }
            self.link(node, height);
        }
    }

    /// The next active node of whichever end of the heights is being taken
    /// from, if there is one.
    fn next_active(&mut self) -> Option<NodeID> {
        loop {
            let at = if self.lowest_label {
                while self.bucket_head[self.lowest] == NOWHERE {
                    if self.lowest + 1 >= self.bucket_head.len() {
                        return None;
                    }
                    self.lowest += 1;
                }
                self.lowest
            } else {
                while self.bucket_head[self.highest] == NOWHERE {
                    if self.highest == 0 {
                        return None;
                    }
                    self.highest -= 1;
                }
                self.highest
            };
            let node = self.bucket_head[at] as NodeID;
            self.bucket_head[at] = self.bucket_next[node];
            if cfg!(debug_assertions) {
                self.bucket_next[node] = NOWHERE;
            }
            // buckets are left stale rather than searched through, so what
            // comes out of one is checked here
            if self.excess[node] > 0
                && (self.height[node] as usize) == at
                && node != self.source
                && node != self.target
            {
                return Some(node);
            }
        }
    }

    /// The next admissible arc out of a node, moving its cursor up to it.
    fn admissible(&mut self, node: NodeID) -> Option<EdgeID> {
        let end = self.residual_graph.end_edges(node);
        while self.current[node] < end {
            let edge = self.current[node];
            let other = self.residual_graph.target(edge);
            if self.residual_graph.data(edge).capacity > 0
                && self.height[node] == self.height[other] + 1
            {
                return Some(edge);
            }
            self.current[node] += 1;
            self.work += 1;
        }
        None
    }

    /// Moves what the walked path will carry along the whole of it.
    fn augment(&mut self, from: NodeID) {
        let mut amount = self.excess[from];
        for &edge in &self.path {
            amount = amount.min(i64::from(self.residual_graph.data(edge).capacity));
        }
        debug_assert!(amount > 0, "an augmentation that moves nothing");
        let moved = i32::try_from(amount).expect("an augmentation larger than a capacity");

        let mut last = from;
        for at in 0..self.path.len() {
            let edge = self.path[at];
            let residual = self.residual_graph.data_mut(edge);
            residual.capacity -= moved;
            residual.reverse_capacity += moved;
            let back = self.pair[edge] as EdgeID;
            let residual = self.residual_graph.data_mut(back);
            residual.capacity += moved;
            residual.reverse_capacity -= moved;
            last = self.residual_graph.target(edge);
            self.pushes += 1;
        }

        self.excess[from] -= amount;
        let before = self.excess[last];
        self.excess[last] += amount;
        if before == 0 && last != self.source && last != self.target {
            self.link(last, self.height[last] as usize);
        }
    }

    /// Carries a node's excess onward a path at a time.
    fn discharge(&mut self, node: NodeID) {
        let unreachable = self.unreachable;
        while self.excess[node] > 0 {
            self.path.clear();
            let mut at = node;
            while self.path.len() < PATH_ARCS && at != self.target {
                let Some(edge) = self.admissible(at) else {
                    break;
                };
                self.path.push(edge);
                at = self.residual_graph.target(edge);
            }

            if self.path.is_empty() {
                self.relabel(node);
                if self.height[node] >= unreachable {
                    return;
                }
                self.current[node] = self.residual_graph.begin_edges(node);
                continue;
            }
            self.augment(node);
        }
    }
}

impl MaxFlow for PushRelabel {
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

        Self {
            residual_graph,
            pair,
            source,
            target,
            unreachable: u32::try_from(number_of_nodes).expect("the graph is too large"),
            max_flow: 0,
            finished: false,
            bound: None,
            height: vec![0; number_of_nodes],
            excess: vec![0; number_of_nodes],
            current: vec![0; number_of_nodes],
            // a height of `n` means the sink cannot be reached, so the counts
            // only ever run up to there
            at_height: vec![0; number_of_nodes + 1],
            bucket_head: vec![NOWHERE; number_of_nodes + 1],
            bucket_next: vec![NOWHERE; number_of_nodes],
            highest: 0,
            lowest: 0,
            // Lowest label by default. Highest label has the better bound and
            // is the usual choice, but a height is the distance to the sink, so
            // it discharges what is furthest from the sink first and delivers
            // nothing until the end -- which makes an upper bound useless. On
            // the cuts measured here lowest label also finished with fewer
            // pushes and far fewer relabels, so nothing is given up for it.
            lowest_label: true,
            queue: VecDeque::with_capacity(number_of_nodes),
            relabels: 0,
            global_relabels: 0,
            pushes: 0,
            work: 0,
            path: Vec::with_capacity(PATH_ARCS),
        }
    }

    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>) {
        debug!("upper bound: {}", bound.load(Ordering::Relaxed));
        self.bound = Some(bound);
        self.run();
    }

    fn run(&mut self) {
        debug!(
            "residual graph size: V {}, E {}",
            self.residual_graph.number_of_nodes(),
            self.residual_graph.number_of_edges()
        );
        let number_of_nodes = self.residual_graph.number_of_nodes();
        if self.source == self.target || number_of_nodes == 0 {
            self.max_flow = 0;
            self.finished = true;
            return;
        }

        self.height.iter_mut().for_each(|height| *height = 0);
        self.excess.iter_mut().for_each(|excess| *excess = 0);
        self.at_height.iter_mut().for_each(|count| *count = 0);
        // Only the heads. A stale `next` is never walked into, since `link`
        // overwrites it before the node is ever at the head of a chain; the
        // clear below is what the assertion in `link` reads, and a sweep of the
        // nodes is not worth paying for in a build that does not check it.
        self.bucket_head.iter_mut().for_each(|head| *head = NOWHERE);
        if cfg!(debug_assertions) {
            self.bucket_next.iter_mut().for_each(|next| *next = NOWHERE);
        }
        self.highest = 0;

        // every arc out of the source is filled, which is where the preflow
        // comes from
        let unreachable = self.unreachable;
        self.height[self.source] = unreachable;
        for edge in self.residual_graph.edge_range(self.source) {
            let moved = self.residual_graph.data(edge).capacity;
            if moved <= 0 {
                continue;
            }
            let other = self.residual_graph.target(edge);
            let residual = self.residual_graph.data_mut(edge);
            residual.capacity -= moved;
            residual.reverse_capacity += moved;
            let back = self.pair[edge] as EdgeID;
            let residual = self.residual_graph.data_mut(back);
            residual.capacity += moved;
            residual.reverse_capacity -= moved;
            self.excess[other] += i64::from(moved);
            self.excess[self.source] -= i64::from(moved);
        }

        if number_of_nodes > SMALLEST_FOR_A_SWEEP {
            // the heights start out exact rather than at zero
            self.global_relabel();
        } else {
            self.seed();
        }

        let sweep = (WORK_PER_SWEEP * self.residual_graph.number_of_edges()).max(1);
        while let Some(node) = self.next_active() {
            self.discharge(node);

            if let Some(bound) = &self.bound {
                // the sink never gives anything back, so what has arrived at it
                // only grows and a bound it has passed it cannot come back under
                let arrived = self.excess[self.target];
                if arrived > i64::from(bound.load(Ordering::Relaxed)) {
                    debug!("aborting max flow computation at {arrived}");
                    self.max_flow = i32::try_from(arrived).unwrap_or(i32::MAX);
                    return;
                }
            }

            if self.work >= sweep {
                self.global_relabel();
            }
        }

        let flow = i32::try_from(self.excess[self.target]).expect("a flow larger than four bytes");
        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        self.max_flow = flow;
        self.finished = true;
    }

    fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }
        debug!(
            "finished in {} pushes, {} relabels and {} global relabels",
            self.pushes, self.relabels, self.global_relabels
        );
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }

        // The side of the cut the source is on is what *cannot reach the sink*,
        // and not what the source reaches.
        //
        // Those are the same set for a flow and not for a preflow. Phase one
        // fills every arc out of the source and leaves standing at the nodes in
        // between whatever never made it to the sink -- phase two, which is not
        // run, is what would send that back. So the arcs out of the source are
        // saturated whatever the flow really is, and a search forward from it
        // would hand back the source on its own and call its own arcs the cut.
        //
        // Searching backwards from the sink has no such trouble. Phase one ends
        // when no node that can still reach the sink holds anything, so no
        // residual path from the source to the sink is left, the two are on
        // opposite sides, and the arcs between the sets cost what arrived.
        let number_of_nodes = self.residual_graph.number_of_nodes();
        let mut reaches_sink = BitVec::new();
        reaches_sink.resize(number_of_nodes, false);
        let mut stack = Vec::with_capacity(number_of_nodes);
        stack.push(self.target);
        reaches_sink.set(self.target, true);
        while let Some(node) = stack.pop() {
            for edge in self.residual_graph.edge_range(node) {
                let other = self.residual_graph.target(edge);
                // the arc that runs the other way is the one that would carry
                // something into `node`, and this one caches its capacity
                if !reaches_sink[other] && self.residual_graph.data(edge).reverse_capacity > 0 {
                    stack.push(other);
                    reaches_sink.set(other, true);
                }
            }
        }

        let mut assignment = !reaches_sink;
        assignment.truncate(number_of_nodes);
        debug_assert!(
            assignment[source],
            "the source can reach the sink after a finished run"
        );
        Ok(assignment)
    }
}

#[cfg(test)]
mod tests {
    use super::PushRelabel;
    use crate::dinic::Dinic;
    use crate::edge::InputEdge;
    use crate::max_flow::{MaxFlow, ResidualEdgeData};
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// What the arcs of the input cost to cut, given which side each node fell
    /// on. This is the check that matters: a flow value nobody can place a cut
    /// against says nothing.
    fn cut_of(edges: &[InputEdge<ResidualEdgeData>], assignment: &bitvec::vec::BitVec) -> i32 {
        edges
            .iter()
            .filter(|edge| assignment[edge.source] && !assignment[edge.target])
            .map(|edge| edge.data.capacity)
            .sum()
    }

    /// A random layered graph. Its depth forces the search through a number of
    /// heights, and the arcs that skip and lead back a layer keep the layering
    /// from being the one the graph was built with.
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
                if layer + 2 < depth && rng.random_range(0..100) < 20 {
                    edges.push(InputEdge::new(
                        node(layer, index),
                        node(layer + 2, index),
                        ResidualEdgeData::new(rng.random_range(1..=5)),
                    ));
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
        let mut solver = PushRelabel::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(3));
        let assignment = solver.assignment(0).expect("it ran");
        assert_eq!(cut_of(&edges, &assignment), 3);
    }

    #[test]
    fn two_ways_round_carry_the_sum_of_both() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(0, 2, ResidualEdgeData::new(6)),
            InputEdge::new(1, 3, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = PushRelabel::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(10));
        let assignment = solver.assignment(0).expect("it ran");
        assert_eq!(cut_of(&edges, &assignment), 10);
    }

    #[test]
    fn a_sink_the_source_cannot_reach_carries_nothing() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = PushRelabel::from_edge_list(edges.clone(), 0, 3);
        solver.run();
        assert_eq!(solver.max_flow(), Ok(0));
        let assignment = solver.assignment(0).expect("it ran");
        assert_eq!(cut_of(&edges, &assignment), 0);
    }

    /// The answer has to be the one an augmenting-path method gives, on graphs
    /// neither of them was written against.
    #[test]
    fn it_agrees_with_dinic_over_many_graphs() {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        for round in 0..64 {
            let width = 2 + round % 7;
            let depth = 2 + round % 5;
            let (edges, source, target) = layered_graph(&mut rng, width, depth);

            let mut pushed = PushRelabel::from_edge_list(edges.clone(), source, target);
            // both ways of picking the next node, since they are two orders over
            // the same invariants and either could be the one that breaks them
            pushed.by_lowest_label(round % 2 == 0);
            pushed.run();
            let mut dinic = Dinic::from_edge_list(edges.clone(), source, target);
            dinic.run();

            let one = pushed.max_flow().expect("push-relabel did not run");
            let other = dinic.max_flow().expect("dinic did not run");
            assert_eq!(
                one, other,
                "round {round}: push-relabel says {one}, dinic says {other}"
            );

            // and the cut it names has to cost what it says the flow is
            let assignment = pushed.assignment(source).expect("push-relabel did not run");
            assert!(assignment[source], "the source is not on its own side");
            assert!(!assignment[target], "the sink is on the source side");
            assert_eq!(
                cut_of(&edges, &assignment),
                one,
                "round {round}: the cut does not cost what the flow does"
            );
        }
    }

    /// A bound the flow passes stops the run, which is how inertial flow drops
    /// an axis that cannot win.
    #[test]
    fn a_flow_over_the_bound_gives_up() {
        use std::sync::{Arc, atomic::AtomicI32};

        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(0, 2, ResidualEdgeData::new(6)),
            InputEdge::new(1, 3, ResidualEdgeData::new(4)),
            InputEdge::new(2, 3, ResidualEdgeData::new(6)),
        ];
        let mut solver = PushRelabel::from_edge_list(edges, 0, 3);
        solver.run_with_upper_bound(Arc::new(AtomicI32::new(2)));
        assert!(
            solver.max_flow().is_err(),
            "a run that gave up reported a flow"
        );
    }

    /// And one it stays under runs to the end and lowers the bound.
    #[test]
    fn a_flow_under_the_bound_lowers_it() {
        use std::sync::{
            Arc,
            atomic::{AtomicI32, Ordering},
        };

        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(4)),
            InputEdge::new(1, 2, ResidualEdgeData::new(4)),
        ];
        let bound = Arc::new(AtomicI32::new(100));
        let mut solver = PushRelabel::from_edge_list(edges, 0, 2);
        solver.run_with_upper_bound(bound.clone());
        assert_eq!(solver.max_flow(), Ok(4));
        assert_eq!(bound.load(Ordering::Relaxed), 4);
    }
}
