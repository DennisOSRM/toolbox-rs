//! Turning the way a search over the cells found into the way through the
//! graph.
//!
//! # What a packed path is
//!
//! A search over the cells steps from one side of a cell straight to the
//! other, at the cost the customization worked out for that pair of border
//! nodes, and never looks at what is inside. So what it hands back is not a
//! way through the graph: between two of its nodes there may be no arc at all,
//! only the promise that the cell can be crossed between them at that cost.
//! Those are the steps this puts back.
//!
//! # Which steps have to be put back
//!
//! Two sets of arcs leave a node the search stepped over at a level: the ones
//! across its cell, which the customization tabulated, and the ones out of the
//! cell, which are arcs of the graph. The first reach border nodes of the same
//! cell and the second only ever leave it, so which of the two a step was is
//! read off the cells of its ends: a step whose ends share a cell at the level
//! its tail was stepped over is a step across that cell, and every other step
//! is an arc of the graph already.
//!
//! Nothing is written down during the search to say so. That is the point: the
//! search runs the same as it did before this existed, and the question is
//! asked of the partition afterwards, once per step of a path of a few hundred
//! rather than once per arc of a search of thousands.
//!
//! # How a step across a cell is put back
//!
//! By searching for it: the same cell, one level down. A step across a cell of
//! level `l` is a path within that cell over the cells of level `l - 1`, which
//! is what the table was built out of in the first place, so a search confined
//! to the cell and stepping over the level below finds a way of exactly the
//! cost the table promised. The steps of *that* path across cells of level
//! `l - 1` are put back the same way, and so on down to the finest level,
//! where a cell is searched over the arcs of the graph and there is nothing
//! left to unpack.
//!
//! This is what OSRM's `unpackPath` does, where the search it recurses into is
//! restricted with a fixed level and a parent cell.

use std::{cmp::Reverse, collections::BinaryHeap};

use rustc_hash::FxHashMap;

use crate::{
    graph::{Graph, NodeID},
    level_directory::CellId,
    lru::LRU,
    overlay::{CellTable, Overlay},
};

/// What a held way costs beyond its nodes: the key it is filed under, the
/// vector heading its nodes, and what the cache and its list keep per entry.
const PER_WAY: usize = size_of::<(NodeID, NodeID, usize)>() + size_of::<Vec<NodeID>>() + 64;

/// Room for the ways of an instance, in bytes.
///
/// # What it scales with
///
/// What the cache is asked to hold is the steps across cells, and there are
/// about as many of those as there are nodes, once per level. A cell of `s`
/// nodes has on the order of the square root of `s` on its border, so it
/// offers about `s` steps across it, and a level holds `n / s` such cells: the
/// size of a cell cancels, and each level offers about as many steps as the
/// graph has nodes. So the room wanted goes with the nodes and the levels, and
/// not with how the levels were cut -- which is worth saying, since it is the
/// first thing one reaches for.
///
/// The arcs come into it only through how wide a border is, which is to say
/// through the constant and not through the shape of the formula.
///
/// # The constant
///
/// Five bytes for every sixteen of those, which is what gives a continent of
/// eighteen million nodes over six levels a cache of thirty two mebibytes.
/// There is nothing deeper in the five and the sixteen than that: it is the
/// room a continent is thought worth giving, worked back into a rate.
///
/// Measured over four thousand eight hundred queries drawn from every rank of
/// a continent, the ways held came to twenty five mebibytes and nothing was
/// ever let go of, so this room is not what binds such a run. It is set for a
/// stream of queries longer or more scattered than that one, and a workload
/// known to be neither should say so with
/// [`Unpacker::with_budget`](Unpacker::with_budget).
#[must_use]
pub fn budget_for<O: Overlay>(overlay: &O) -> usize {
    overlay
        .graph()
        .number_of_nodes()
        .saturating_mul(overlay.levels())
        / 16
        * 5
}

/// What went wrong turning a packed path into a way through the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unpacking {
    /// A step across a cell that the cell turned out not to offer. A table
    /// saying a cell can be crossed where it cannot is a customization that
    /// does not describe the graph.
    NoWayAcross {
        from: NodeID,
        to: NodeID,
        level: usize,
    },
    /// A step that is neither an arc of the graph nor a step across a cell.
    NotAStep { from: NodeID, to: NodeID },
}

/// The way through the graph that a packed path stands for.
///
/// `packed` is what a search over the cells handed back, from source to
/// target. What comes back is every node of the way, so that each pair of
/// neighbours in it is an arc of the graph.
///
/// # Errors
///
/// Returns the first step that could not be put back, which means the tables
/// and the graph disagree.
///
/// # Panics
///
/// Panics if the packed path holds a node the partition does not.
pub fn unpack<O: Overlay>(overlay: &O, packed: &[NodeID]) -> Result<Vec<NodeID>, Unpacking> {
    Unpacker::default().unpack(overlay, packed)
}

/// An unpacker that remembers the ways it has already put back.
///
/// A step across a coarse cell is one of a few hundred that cell offers, and
/// every path crossing that part of the continent takes one of them. Putting
/// one back is a search, so the second query to cross it pays for the same
/// search again. Held here, it is a lookup.
///
/// What is kept is the finished way, all the way down to the arcs, so a hit at
/// a coarse level saves the whole recursion under it rather than one level of
/// it. Nothing is thrown away: a run over a continent's worth of queries keeps
/// what it has crossed, and a caller that wants the room back asks for it with
/// [`clear`](Self::clear).
pub struct Unpacker {
    ways: LRU<(NodeID, NodeID, usize), Vec<NodeID>>,
    /// what the ways held come to, kept as they go in and out rather than
    /// walked, since it is asked after every insert
    bytes: usize,
    budget: usize,
    hits: usize,
    misses: usize,
    evicted: usize,
}

impl Default for Unpacker {
    /// An unpacker with room enough for a path or two, which is what
    /// [`unpack`](unpack) makes and throws away again.
    ///
    /// A caller putting back a file of them wants
    /// [`for_instance`](Self::for_instance), which holds what the instance
    /// warrants; one that knows better says so with
    /// [`with_budget`](Self::with_budget).
    fn default() -> Self {
        Self::with_budget(1 << 20)
    }
}

impl Unpacker {
    /// An unpacker holding what the instance warrants, by
    /// [`budget_for`](budget_for).
    #[must_use]
    pub fn for_instance<O: Overlay>(overlay: &O) -> Self {
        Self::with_budget(budget_for(overlay))
    }

    /// An unpacker holding no more than the given number of bytes of ways.
    ///
    /// Held to the room rather than to a count of ways: a way put back at the
    /// coarsest level is hundreds of nodes and one at the finest is two, so a
    /// count says little about what is being kept.
    ///
    /// # Panics
    ///
    /// Panics if the room asked for is more than a table can be made for. The
    /// table is made once, to the size the room allows, so that nothing is
    /// rehashed later; a budget of everything there is has no table.
    #[must_use]
    pub fn with_budget(bytes: usize) -> Self {
        // The list is held to a count as well, as that is what it is built
        // around. Set from the room and the smallest a way can be, so that it
        // is the room that binds and not this.
        let entries = (bytes / (PER_WAY + 2 * size_of::<NodeID>())).max(1);
        Self {
            ways: LRU::new_with_capacity(entries),
            bytes: 0,
            budget: bytes,
            hits: 0,
            misses: 0,
            evicted: 0,
        }
    }

    /// how many steps across a cell were answered from what was already here
    #[must_use]
    pub const fn hits(&self) -> usize {
        self.hits
    }

    /// how many had to be searched for
    #[must_use]
    pub const fn misses(&self) -> usize {
        self.misses
    }

    /// how many ways were let go of to make room
    #[must_use]
    pub const fn evicted(&self) -> usize {
        self.evicted
    }

    /// the room the cache was given
    #[must_use]
    pub const fn budget(&self) -> usize {
        self.budget
    }

    /// how many ways are being held
    #[must_use]
    pub fn len(&self) -> usize {
        self.ways.len()
    }

    /// What the ways held come to in bytes: their nodes, and what each costs
    /// to keep beyond them.
    #[must_use]
    pub const fn held_bytes(&self) -> usize {
        self.bytes
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ways.is_empty()
    }

    /// Drops what is held, keeping the counts.
    pub fn clear(&mut self) {
        self.ways.clear();
        self.bytes = 0;
    }

    /// Files a way, letting go of whatever was used longest ago until what is
    /// held is back inside the room it was given.
    fn keep(&mut self, key: (NodeID, NodeID, usize), way: &[NodeID]) {
        let cost = PER_WAY + size_of_val(way);
        if cost > self.budget {
            // a way that would not leave room for itself is not worth holding
            return;
        }
        self.bytes += cost;
        if let Some((_, gone)) = self.ways.push(&key, way.to_vec()) {
            self.bytes -= PER_WAY + gone.len() * size_of::<NodeID>();
            self.evicted += 1;
        }
        while self.bytes > self.budget {
            let Some((_, gone)) = self.ways.pop_lru() else {
                break;
            };
            self.bytes -= PER_WAY + gone.len() * size_of::<NodeID>();
            self.evicted += 1;
        }
    }

    /// The way through the graph that a packed path stands for.
    ///
    /// # Errors
    ///
    /// Returns the first step that could not be put back.
    ///
    /// # Panics
    ///
    /// Panics if the packed path holds a node the partition does not.
    pub fn unpack<O: Overlay>(
        &mut self,
        overlay: &O,
        packed: &[NodeID],
    ) -> Result<Vec<NodeID>, Unpacking> {
        self.unpack_inner(overlay, packed)
    }

    fn unpack_inner<O: Overlay>(
        &mut self,
        overlay: &O,
        packed: &[NodeID],
    ) -> Result<Vec<NodeID>, Unpacking> {
        let Some((&source, rest)) = packed.split_first() else {
            return Ok(Vec::new());
        };
        let partition = overlay.partition();
        let source_word = partition.word(source);
        let target_word = partition.word(*packed.last().expect("the path has a first node"));

        let mut way = vec![source];
        let mut from = source;
        for &to in rest {
            // the level the tail was stepped over at is the level a step across a
            // cell would have been taken at, and the same question the search
            // asked of it
            match partition.query_level(source_word, target_word, from) {
                Some(level)
                    if partition.same_cell_at(partition.word(from), partition.word(to), level) =>
                {
                    let across = self.across_cell(overlay, from, to, level)?;
                    way.extend_from_slice(&across[1..]);
                }
                _ => way.push(to),
            }
            from = to;
        }
        Ok(way)
    }

    /// A way from one border node of a cell to another, inside the cell.
    ///
    /// Every node of it, so the caller may lay it end to end with what it already
    /// has. The first node is `from` and the last is `to`.
    fn across_cell<O: Overlay>(
        &mut self,
        overlay: &O,
        from: NodeID,
        to: NodeID,
        level: usize,
    ) -> Result<Vec<NodeID>, Unpacking> {
        if let Some(held) = self.ways.get(&(from, to, level)) {
            self.hits += 1;
            return Ok(held.clone());
        }
        self.misses += 1;
        let partition = overlay.partition();
        let cell = partition.cell_of(from, level);
        let found = within_cell(overlay, from, to, level, cell).ok_or(Unpacking::NoWayAcross {
            from,
            to,
            level,
        })?;

        // The way found is over the cells of the level below, so its own steps
        // across those cells are put back the same way. At the finest level there
        // is no level below and every step is already an arc.
        if level == 0 {
            self.keep((from, to, level), &found);
            return Ok(found);
        }
        let below = level - 1;
        let mut way = vec![found[0]];
        for pair in found.windows(2) {
            let (step_from, step_to) = (pair[0], pair[1]);
            if partition.same_cell_at(partition.word(step_from), partition.word(step_to), below) {
                let across = self.across_cell(overlay, step_from, step_to, below)?;
                way.extend_from_slice(&across[1..]);
            } else {
                way.push(step_to);
            }
        }
        self.keep((from, to, level), &way);
        Ok(way)
    }
}

/// A shortest way from one node of a cell to another without leaving it,
/// stepping over the cells of the level below.
///
/// This is the search the table of the cell was built out of, run for one pair
/// rather than for all of them, and asked for the way rather than the cost.
/// The nodes it walks are the border nodes of the cells below plus whatever
/// arcs of the graph run between them, which on a coarse cell is a small part
/// of what it holds.
fn within_cell<O: Overlay>(
    overlay: &O,
    from: NodeID,
    to: NodeID,
    level: usize,
    cell: CellId,
) -> Option<Vec<NodeID>> {
    if from == to {
        return Some(vec![from]);
    }
    let partition = overlay.partition();
    let graph = overlay.graph();

    let mut parent: FxHashMap<NodeID, NodeID> = FxHashMap::default();
    let mut best: FxHashMap<NodeID, usize> = FxHashMap::default();
    let mut settled: Vec<NodeID> = Vec::new();
    let mut queue = BinaryHeap::new();
    parent.insert(from, from);
    best.insert(from, 0);
    queue.push(Reverse((0_usize, from)));

    while let Some(Reverse((cost, node))) = queue.pop() {
        // a lazy heap: a node is offered as often as it is reached and the
        // stale offers are dropped here rather than lowered in place
        if cost > best.get(&node).copied().unwrap_or(usize::MAX) {
            continue;
        }
        if settled.contains(&node) {
            continue;
        }
        settled.push(node);
        if node == to {
            let mut way = vec![to];
            let mut at = to;
            while parent[&at] != at {
                at = parent[&at];
                way.push(at);
            }
            way.reverse();
            return Some(way);
        }

        // Where a node is reached more cheaply than before, both what it is
        // held at and where it was reached from move together. Writing the one
        // without the other is a way that is a way, and costs more than the
        // one the search actually found.
        let mut offer = |target: NodeID, weight: usize| {
            if weight < best.get(&target).copied().unwrap_or(usize::MAX) {
                best.insert(target, weight);
                parent.insert(target, node);
                queue.push(Reverse((weight, target)));
            }
        };

        if level > 0 {
            // across the cells of the level below, which is what the table of
            // this cell was built out of
            let below = level - 1;
            let inner = partition.cell_of(node, below);
            if let Some(distances) = overlay.distances_of(below, inner)
                && let Some(place) = distances.place_of(node)
            {
                for (&other, &across) in distances.border_nodes().iter().zip(distances.row(place)) {
                    if across == u32::MAX || other as NodeID == node {
                        continue;
                    }
                    offer(other as NodeID, cost + across as usize);
                }
            }
        }

        // and the arcs of the graph, which is how the search gets from one cell
        // of the level below into the next, and all it has at the finest level
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            // never leave the cell this is confined to
            if partition.cell_of(target, level) != cell {
                continue;
            }
            if level > 0
                && partition.same_cell_at(partition.word(node), partition.word(target), level - 1)
            {
                // inside a cell of the level below, which the table above
                // answered for in one step
                continue;
            }
            offer(target, cost + *graph.data(edge) as usize);
        }
    }
    None
}

/// What a way through the graph costs, and `None` if it is not a way at all.
///
/// This is what says an unpacked path is worth having: laid against what the
/// query said the way costs, it is the whole of the check.
///
/// # Panics
///
/// Panics if the way holds a node the graph does not.
#[must_use]
pub fn cost_of_way<G: Graph<u32>>(graph: &G, way: &[NodeID]) -> Option<usize> {
    let mut total = 0_usize;
    for pair in way.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let arc = graph
            .edge_range(from)
            .filter(|&edge| graph.target(edge) == to)
            .map(|edge| *graph.data(edge) as usize)
            .min()?;
        total += arc;
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bidirectional_mld_query::BidirectionalMldQuery,
        border_levels::BorderLevels,
        customization::Customization,
        edge::InputEdge,
        grid_graph::{grid_directory, grid_edges},
        mld_query::MldQuery,
        static_graph::StaticGraph,
    };
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    fn reversed(edges: &[InputEdge<u32>]) -> StaticGraph<u32> {
        StaticGraph::new(
            edges
                .iter()
                .map(|edge| InputEdge::new(edge.target, edge.source, edge.data))
                .collect(),
        )
    }

    /// An unpacked path has to be a way the graph really offers, and it has to
    /// cost what the query said the way costs. Either alone is not enough: a
    /// path of the right cost that steps where there is no arc is not a way,
    /// and a way that costs more than the query promised is not the way it
    /// found.
    fn every_pair_unpacks(side: usize, seed: u64) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut edges = grid_edges(side, true);
        for edge in &mut edges {
            edge.data = rng.random_range(1..25_u32);
        }
        let plain = StaticGraph::new(edges.clone());
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

        let count = side * side;
        let mut query = MldQuery::new();
        for _ in 0..40 {
            let source = rng.random_range(0..count as NodeID);
            let target = rng.random_range(0..count as NodeID);
            query.run(&customization, source, &[target]);
            let Some(packed) = query.retrieve_packed_path(target) else {
                continue;
            };
            assert_eq!(packed.first(), Some(&source));
            assert_eq!(packed.last(), Some(&target));

            let way = unpack(&customization, &packed).expect("the cells offer what they said");
            assert_eq!(way.first(), Some(&source), "{source} to {target}");
            assert_eq!(way.last(), Some(&target), "{source} to {target}");
            let walked = cost_of_way(&plain, &way)
                .unwrap_or_else(|| panic!("{source} to {target}: {way:?} is not a way"));
            assert_eq!(
                walked,
                query.distance(target),
                "{source} to {target}: the way costs {walked} against the {} reported",
                query.distance(target)
            );
        }
    }

    #[test]
    fn a_grid_of_three_levels_unpacks() {
        every_pair_unpacks(16, 0x_11AC);
    }

    /// Six levels, so a step is put back through five levels of cell before it
    /// reaches an arc.
    #[test]
    fn a_grid_of_six_levels_unpacks() {
        every_pair_unpacks(64, 0x_51E5);
    }

    /// The search from both ends packs its path out of two queues, so its
    /// steps want putting back just the same.
    #[test]
    fn a_path_from_both_ends_unpacks() {
        let side = 32;
        let mut rng = StdRng::seed_from_u64(0x_B07E);
        let mut edges = grid_edges(side, true);
        for edge in &mut edges {
            edge.data = rng.random_range(1..25_u32);
        }
        let plain = StaticGraph::new(edges.clone());
        let reverse = reversed(&edges);
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));
        let backward = BorderLevels::of(&reverse, customization.partition());

        let count = side * side;
        let mut query = BidirectionalMldQuery::new();
        for _ in 0..40 {
            let source = rng.random_range(0..count as NodeID);
            let target = rng.random_range(0..count as NodeID);
            let reported = query.run(&customization, &reverse, &backward, source, target);
            if reported == usize::MAX {
                continue;
            }
            let packed = query.retrieve_packed_path().expect("a way was found");
            assert_eq!(packed.first(), Some(&source));
            assert_eq!(packed.last(), Some(&target));

            let way = unpack(&customization, &packed).expect("the cells offer what they said");
            let walked = cost_of_way(&plain, &way)
                .unwrap_or_else(|| panic!("{source} to {target}: {way:?} is not a way"));
            assert_eq!(walked, reported, "{source} to {target}");
        }
    }

    /// What is held has to be what would have been searched for, or a second
    /// query over the same ground gets a different answer from the first.
    #[test]
    fn a_held_way_is_the_way_that_would_have_been_found() {
        let side = 32;
        let mut rng = StdRng::seed_from_u64(0x_CAC4E);
        let mut edges = grid_edges(side, true);
        for edge in &mut edges {
            edge.data = rng.random_range(1..25_u32);
        }
        let plain = StaticGraph::new(edges.clone());
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

        let count = side * side;
        let mut query = MldQuery::new();
        let mut held = Unpacker::for_instance(&customization);
        for _ in 0..60 {
            let source = rng.random_range(0..count as NodeID);
            let target = rng.random_range(0..count as NodeID);
            query.run(&customization, source, &[target]);
            let Some(packed) = query.retrieve_packed_path(target) else {
                continue;
            };
            let fresh = unpack(&customization, &packed).expect("the cells offer what they said");
            let cached = held
                .unpack(&customization, &packed)
                .expect("the cells offer what they said");
            assert_eq!(fresh, cached, "{source} to {target}");
            assert_eq!(
                cost_of_way(&plain, &cached),
                Some(query.distance(target)),
                "{source} to {target}"
            );
        }
        // a grid of six hundred queries crosses the same cells over and over,
        // so something has to have been answered from what was already held
        assert!(held.hits() > 0, "nothing was ever held");
        assert!(!held.is_empty());
    }

    /// The room is a bound, not a wish: an unpacker given very little holds
    /// very little, lets go of the rest, and still answers correctly.
    #[test]
    fn a_small_budget_is_kept_to() {
        let side = 32;
        let mut rng = StdRng::seed_from_u64(0x_B0FF);
        let mut edges = grid_edges(side, true);
        for edge in &mut edges {
            edge.data = rng.random_range(1..25_u32);
        }
        let plain = StaticGraph::new(edges.clone());
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

        let budget = 8 * 1024;
        let mut held = Unpacker::with_budget(budget);
        let count = side * side;
        let mut query = MldQuery::new();
        for _ in 0..80 {
            let source = rng.random_range(0..count as NodeID);
            let target = rng.random_range(0..count as NodeID);
            query.run(&customization, source, &[target]);
            let Some(packed) = query.retrieve_packed_path(target) else {
                continue;
            };
            let way = held
                .unpack(&customization, &packed)
                .expect("the cells offer what they said");
            // what it holds never says anything about what it answers
            assert_eq!(
                cost_of_way(&plain, &way),
                Some(query.distance(target)),
                "{source} to {target}"
            );
            assert!(
                held.held_bytes() <= budget,
                "held {} bytes of {budget}",
                held.held_bytes()
            );
        }
        assert!(held.evicted() > 0, "a cache this small should have let go");
    }

    /// The room an instance is given goes with its nodes and its levels, and
    /// a continent gets thirty two mebibytes.
    #[test]
    fn the_budget_goes_with_nodes_and_levels() {
        let small = Customization::new(StaticGraph::new(grid_edges(16, true)), grid_directory(16));
        let large = Customization::new(StaticGraph::new(grid_edges(64, true)), grid_directory(64));
        assert!(budget_for(&large) > budget_for(&small));

        // eighteen million nodes over six levels, which is what a continent
        // came to, wants thirty two mebibytes give or take a percent
        let continent = 18_010_173_usize * 6 / 16 * 5;
        let mebibytes = continent as f64 / (1024.0 * 1024.0);
        assert!(
            (mebibytes - 32.0).abs() < 1.0,
            "a continent would be given {mebibytes:.1} MiB"
        );
    }

    /// A packed path of one node is a query that asked about a node and
    /// itself, and unpacks to itself.
    #[test]
    fn a_path_of_one_node_unpacks_to_itself() {
        let customization =
            Customization::new(StaticGraph::new(grid_edges(8, true)), grid_directory(8));
        assert_eq!(unpack(&customization, &[7]), Ok(vec![7]));
        assert_eq!(unpack(&customization, &[]), Ok(Vec::new()));
    }

    /// Unpacking puts nodes back, so an unpacked way is at least as long as
    /// the packed one, and on a grid of several levels it is longer.
    #[test]
    fn a_step_over_a_cell_becomes_more_than_one_arc() {
        let side = 32;
        let mut rng = StdRng::seed_from_u64(0x_57E9);
        let mut edges = grid_edges(side, true);
        for edge in &mut edges {
            edge.data = rng.random_range(1..25_u32);
        }
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

        let mut query = MldQuery::new();
        let (source, target) = (0, (side * side - 1) as NodeID);
        query.run(&customization, source, &[target]);
        let packed = query.retrieve_packed_path(target).expect("a way was found");
        let way = unpack(&customization, &packed).expect("the cells offer what they said");
        assert!(
            way.len() > packed.len(),
            "a corner to corner way over {} levels unpacked {} nodes into {}",
            customization.directory().levels(),
            packed.len(),
            way.len()
        );
    }
}
