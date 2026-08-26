//! The nearest node, or the nearest piece of road, to a place on the map.
//!
//! # What this is, beside [`RTree`](crate::r_tree::RTree)
//!
//! There is already an R-tree here, and it already browses by distance. What
//! it does not do is page: it holds a vector of nodes and a vector of leaves,
//! which is the thing an offline instance cannot afford. This is the same walk
//! over a tree that is either held or read, the way
//! [`PackedPartition`](crate::packed_partition::PackedPartition) and
//! [`CellTree`](crate::cell_tree::CellTree) are, so one implementation answers
//! for both.
//!
//! It also measures differently, and for a reason. `RTree` is generic over a
//! [`Metric`](crate::metric::Metric), whose default is great-circle, and whose
//! own documentation records that its bound on a box is a flat rectangle's and
//! so can read too long -- which is exactly the contract a browse rests on.
//! Here everything is measured in one scaled plane, boxes and segments alike,
//! so the bound holds by construction. The two are worth reconciling; they are
//! not reconciled yet.
//!
//! # Distance browsing
//!
//! The search is Hjaltason and Samet's incremental nearest neighbour: a queue
//! holding both the boxes of the tree and the things underneath them, ordered
//! by how far away they are, and a loop that takes the nearest off it. A box
//! is keyed by the distance to the nearest point of it, which is a floor on
//! anything inside; a thing is keyed by how far away it actually is. So the
//! first thing to come off the queue is the nearest thing there is, and the
//! second is the second nearest, without the search having been told in
//! advance how many were wanted.
//!
//! That is the property [`browse`](NearestIndex::browse) hands out. Asking for
//! one is [`nearest`](NearestIndex::nearest), which is the same walk stopped
//! after the first.
//!
//! # A tree with no pointers in it
//!
//! The boxes are an implicit tree, laid out the way a heap is: with a fan-out
//! of `FAN`, the children of node `i` at one level are `i * FAN .. (i + 1) *
//! FAN` at the level below, and the bottom level's children are the things
//! themselves. Nothing stores a child index, because nothing has to: it is
//! arithmetic, which is what lets both the boxes and the things be a
//! [`PagedArray`] and cost nothing standing still.
//!
//! The things are put in [Sort-Tile-Recursive] order before the boxes are
//! built over them, so a box holds things that are near each other rather than
//! things that happen to be numbered together.
//!
//! [Sort-Tile-Recursive]: https://doi.org/10.1109/ICDE.1997.582015
//!
//! # One plane, and why it is scaled
//!
//! A browse is only correct where the key on a box is a floor on the key of
//! everything under it, so the boxes and the things have to be measured the
//! same way. Latitude and longitude are not that measure: a degree of
//! longitude is shorter than a degree of latitude everywhere but the equator,
//! so a plane read straight off the two ranks a thing due east nearer than it
//! is.
//!
//! So longitude is scaled by the cosine of the middle latitude of the data,
//! once, and everything is measured in that plane. Within a continent it is
//! within a percent of the great circle, and the ranking it gives is the
//! ranking the great circle gives. What is handed back is turned into metres
//! at the end, where it is a distance and not a key.

use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::Arc,
};

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    block_store::read_at,
    bounding_box::BoundingBox,
    geometry::{FPCoordinate, Point2D, Segment, distance_to_segment_2d},
    graph::Arcs,
    paged_array::PagedArray,
    pool::Pool,
};

/// The version a written index is under.
pub const VERSION: u16 = 1;

/// How many children a box has, and how many things the bottom row of boxes
/// holds apiece.
///
/// Thirty two keeps the tree five deep over a continent and a box's children
/// inside one read of the array they are in.
pub const FAN: usize = 32;

/// How many fixed-point degrees make a degree, which is what the coordinates
/// are in.
const FIXED: f64 = 1_000_000.0;

/// Roughly how many metres a degree of latitude is.
const METRES_A_DEGREE: f64 = 111_320.0;

/// What the index is over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub enum Kind {
    /// the nodes of the graph, one thing apiece
    Nodes,
    /// the arcs of the graph, as the piece of road between their ends
    Segments,
}

/// One thing the index holds.
///
/// A node is a segment whose ends are the same place, which is what lets one
/// walk serve both: the distance to it comes out of the same call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    /// the node or the arc this is
    pub what: u32,
    pub from: FPCoordinate,
    pub to: FPCoordinate,
}

/// How wide an [`Item`] is written.
const ITEM_BYTES: usize = 4 + 8 + 8;
/// And a [`BoundingBox`].
const BOX_BYTES: usize = 16;

/// What a browse hands back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    /// the node, or the arc
    pub what: u32,
    /// where on it the nearest point is: the node itself, or the place on the
    /// segment the query is beside
    pub at: FPCoordinate,
    /// how far away that is, in metres
    pub away: f64,
}

/// Where the boxes live.
enum Boxes {
    Held(Vec<BoundingBox>),
    Paged(PagedArray),
}

/// And the things.
enum Items {
    Held(Vec<Item>),
    Paged(PagedArray),
}

/// What a written index says about itself before its arrays.
#[derive(Archive, Serialize, Deserialize)]
struct Head {
    version: u16,
    kind: Kind,
    fan: u32,
    objects: u64,
    /// how many boxes each level has, the bottom row first
    counts: Vec<u32>,
    lon_scale: f64,
}

/// A nearest-first index over the nodes or the arcs of a graph.
pub struct NearestIndex {
    kind: Kind,
    fan: usize,
    objects: usize,
    /// where each level begins in the flat run of boxes, the bottom row first
    level_at: Vec<u64>,
    counts: Vec<u32>,
    boxes: Boxes,
    items: Items,
    /// longitude times this is the plane everything is measured in
    lon_scale: f64,
}

impl NearestIndex {
    /// An index over the nodes of a graph.
    ///
    /// # Panics
    ///
    /// Panics for more nodes than four thousand million.
    #[must_use]
    pub fn over_nodes(coordinates: &[FPCoordinate]) -> Self {
        let items = coordinates
            .iter()
            .enumerate()
            .map(|(at, &place)| Item {
                what: u32::try_from(at).expect("a node in four bytes"),
                from: place,
                to: place,
            })
            .collect();
        Self::of(items, Kind::Nodes)
    }

    /// An index over the arcs of a graph, as the road between their ends.
    ///
    /// Only one direction of a two-way road is kept: the two carry the same
    /// piece of road, and a browse that hands out both hands the same answer
    /// twice.
    ///
    /// # Panics
    ///
    /// Panics for more arcs than four thousand million.
    #[must_use]
    pub fn over_segments<G: Arcs<u32>>(graph: &G, coordinates: &[FPCoordinate]) -> Self {
        let mut items = Vec::new();
        for node in graph.node_range() {
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                // the same road twice over, once each way, is one road
                if target <= node {
                    continue;
                }
                items.push(Item {
                    what: u32::try_from(edge).expect("an arc in four bytes"),
                    from: coordinates[node],
                    to: coordinates[target],
                });
            }
        }
        Self::of(items, Kind::Segments)
    }

    /// Builds the tree over things already collected.
    fn of(mut items: Vec<Item>, kind: Kind) -> Self {
        let whole = BoundingBox::from_coordinates(
            &items
                .iter()
                .flat_map(|item| [item.from, item.to])
                .collect::<Vec<_>>(),
        );
        let middle = whole.center();
        let lon_scale = (f64::from(middle.lat) / FIXED).to_radians().cos();

        sort_tile_recursive(&mut items, lon_scale);

        // the bottom row of boxes, one over every FAN things, and then a row
        // over every FAN of those until one is left
        let mut boxes: Vec<BoundingBox> = Vec::new();
        let mut counts: Vec<u32> = Vec::new();
        let mut level_at: Vec<u64> = Vec::new();

        let mut wide = items.len().div_ceil(FAN);
        level_at.push(0);
        counts.push(u32::try_from(wide).expect("a level in four bytes"));
        for at in 0..wide {
            let mut held = BoundingBox::invalid();
            for item in &items[at * FAN..((at + 1) * FAN).min(items.len())] {
                held.extend_with(&BoundingBox::from_coordinates(&[item.from, item.to]));
            }
            boxes.push(held);
        }

        while wide > 1 {
            let below = boxes.len() - wide;
            let above = wide.div_ceil(FAN);
            level_at.push(boxes.len() as u64);
            counts.push(u32::try_from(above).expect("a level in four bytes"));
            for at in 0..above {
                let mut held = BoundingBox::invalid();
                for which in at * FAN..((at + 1) * FAN).min(wide) {
                    held.extend_with(&boxes[below + which]);
                }
                boxes.push(held);
            }
            wide = above;
        }

        Self {
            kind,
            fan: FAN,
            objects: items.len(),
            level_at,
            counts,
            boxes: Boxes::Held(boxes),
            items: Items::Held(items),
            lon_scale,
        }
    }

    /// What it is over.
    #[must_use]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// How many things it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.objects
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects == 0
    }

    /// How many rows of boxes there are over the things.
    #[must_use]
    pub fn levels(&self) -> usize {
        self.counts.len()
    }

    /// What it costs standing still.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
            + (self.level_at.capacity() + self.counts.capacity()) * 8
            + match &self.boxes {
                Boxes::Held(held) => held.capacity() * size_of::<BoundingBox>(),
                Boxes::Paged(read) => read.bytes(),
            }
            + match &self.items {
                Items::Held(held) => held.capacity() * size_of::<Item>(),
                Items::Paged(read) => read.bytes(),
            }
    }

    /// The box at a place in a row.
    fn box_at(&self, level: usize, at: usize) -> BoundingBox {
        let flat = self.level_at[level] as usize + at;
        match &self.boxes {
            Boxes::Held(held) => held[flat],
            Boxes::Paged(read) => {
                let held = read.get::<BOX_BYTES>(flat).expect("a box the index has");
                BoundingBox::between(
                    FPCoordinate::new(word(&held[0..4]), word(&held[4..8])),
                    FPCoordinate::new(word(&held[8..12]), word(&held[12..16])),
                )
            }
        }
    }

    /// The thing at a place.
    fn item_at(&self, at: usize) -> Item {
        match &self.items {
            Items::Held(held) => held[at],
            Items::Paged(read) => {
                let held = read.get::<ITEM_BYTES>(at).expect("a thing the index has");
                Item {
                    what: u32::from_le_bytes(held[0..4].try_into().expect("four bytes")),
                    from: FPCoordinate::new(word(&held[4..8]), word(&held[8..12])),
                    to: FPCoordinate::new(word(&held[12..16]), word(&held[16..20])),
                }
            }
        }
    }

    /// Every thing there is, nearest first.
    ///
    /// The walk is lazy: it does no more work than the caller takes answers,
    /// so asking for the first is not asking for a sort.
    #[must_use]
    pub fn browse(&self, at: FPCoordinate) -> Browsing<'_> {
        let mut queue = BinaryHeap::new();
        if self.objects > 0 {
            let top = self.counts.len() - 1;
            for which in 0..self.counts[top] as usize {
                let box_at = self.box_at(top, which);
                queue.push(Nearer {
                    away: scaled_min_distance(&box_at, at, self.lon_scale),
                    what: Step::Boxes(top, which),
                });
            }
        }
        Browsing {
            over: self,
            at,
            queue,
        }
    }

    /// The nearest thing, and nothing where the index is empty.
    #[must_use]
    pub fn nearest(&self, at: FPCoordinate) -> Option<Found> {
        self.browse(at).next()
    }

    /// Writes the index out for [`open`](Self::open) to read.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong writing it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let (Boxes::Held(boxes), Items::Held(items)) = (&self.boxes, &self.items) else {
            return Err(std::io::Error::other(
                "this index is read off a file and cannot be written back",
            ));
        };
        let head = Head {
            version: VERSION,
            kind: self.kind,
            fan: u32::try_from(self.fan).expect("a fan in four bytes"),
            objects: self.objects as u64,
            counts: self.counts.clone(),
            lon_scale: self.lon_scale,
        };
        let head = rkyv::to_bytes::<rkyv::rancor::Error>(&head)
            .map_err(|why| std::io::Error::other(format!("an index will not serialize: {why}")))?;

        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&(head.len() as u64).to_le_bytes())?;
        out.write_all(&head)?;
        for held in boxes {
            let (min, max) = held.corners();
            out.write_all(&min.lat.to_le_bytes())?;
            out.write_all(&min.lon.to_le_bytes())?;
            out.write_all(&max.lat.to_le_bytes())?;
            out.write_all(&max.lon.to_le_bytes())?;
        }
        for item in items {
            out.write_all(&item.what.to_le_bytes())?;
            out.write_all(&item.from.lat.to_le_bytes())?;
            out.write_all(&item.from.lon.to_le_bytes())?;
            out.write_all(&item.to.lat.to_le_bytes())?;
            out.write_all(&item.to.lon.to_le_bytes())?;
        }
        out.flush()
    }

    /// Reads one back, holding none of it.
    ///
    /// What stands is a count and an offset a level, which is the logarithm of
    /// the things: the boxes and the things come out of the pool as they are
    /// asked for.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong reading it, and refuses a version this does
    /// not know.
    pub fn open(path: &Path, pool: &Arc<Pool>) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let mut length = [0_u8; 8];
        read_at(&file, 0, &mut length)?;
        let length = u64::from_le_bytes(length) as usize;
        let mut head = vec![0_u8; length];
        read_at(&file, 8, &mut head)?;
        let head: Head = rkyv::from_bytes::<Head, rkyv::rancor::Error>(&head)
            .map_err(|why| std::io::Error::other(format!("an index will not read: {why}")))?;
        if head.version != VERSION {
            return Err(std::io::Error::other(format!(
                "an index written under version {}",
                head.version
            )));
        }

        let mut level_at = Vec::with_capacity(head.counts.len());
        let mut running = 0_u64;
        for &count in &head.counts {
            level_at.push(running);
            running += u64::from(count);
        }
        let boxes = running;

        let at = 8 + length as u64;
        Ok(Self {
            kind: head.kind,
            fan: head.fan as usize,
            objects: head.objects as usize,
            level_at,
            counts: head.counts,
            boxes: Boxes::Paged(PagedArray::open(
                path,
                at,
                boxes as usize,
                BOX_BYTES,
                Arc::clone(pool),
            )?),
            items: Items::Paged(PagedArray::open(
                path,
                at + boxes * BOX_BYTES as u64,
                head.objects as usize,
                ITEM_BYTES,
                Arc::clone(pool),
            )?),
            lon_scale: head.lon_scale,
        })
    }
}

/// Four bytes of a written coordinate.
fn word(held: &[u8]) -> i32 {
    i32::from_le_bytes(held.try_into().expect("four bytes"))
}

/// A place in the plane everything is measured in.
fn flat(place: FPCoordinate, lon_scale: f64) -> Point2D {
    Point2D {
        x: f64::from(place.lon) * lon_scale,
        y: f64::from(place.lat),
    }
}

/// How far a box is, in that plane, and nothing where the query is inside it.
fn scaled_min_distance(held: &BoundingBox, at: FPCoordinate, lon_scale: f64) -> f64 {
    let near = held.nearest_point(&at);
    let (a, b) = (flat(near, lon_scale), flat(at, lon_scale));
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// How far a thing is, in that plane, and where on it the near point is.
fn scaled_item_distance(item: &Item, at: FPCoordinate, lon_scale: f64) -> (f64, Point2D) {
    distance_to_segment_2d(
        &flat(at, lon_scale),
        &Segment(flat(item.from, lon_scale), flat(item.to, lon_scale)),
    )
}

/// What is on the queue: a row of boxes, or a thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// a box, by which row it is in and where in the row
    Boxes(usize, usize),
    /// a thing, by where it is, with the near point on it already worked out
    Thing(usize, i32, i32),
}

/// A queue entry, ordered so the nearest comes off first.
#[derive(Clone, Copy, Debug)]
struct Nearer {
    away: f64,
    what: Step,
}

impl PartialEq for Nearer {
    fn eq(&self, other: &Self) -> bool {
        self.away == other.away && self.what == other.what
    }
}
impl Eq for Nearer {}

impl Ord for Nearer {
    fn cmp(&self, other: &Self) -> Ordering {
        // a max-heap is what BinaryHeap is, and what is wanted is the nearest,
        // so the comparison is turned around
        other
            .away
            .partial_cmp(&self.away)
            .unwrap_or(Ordering::Equal)
            // and a thing comes before a box it ties with, since the box would
            // only hand out things at that distance anyway
            .then_with(|| match (self.what, other.what) {
                (Step::Thing(..), Step::Boxes(..)) => Ordering::Greater,
                (Step::Boxes(..), Step::Thing(..)) => Ordering::Less,
                _ => Ordering::Equal,
            })
    }
}
impl PartialOrd for Nearer {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Every thing in an index, nearest to a place first.
pub struct Browsing<'a> {
    over: &'a NearestIndex,
    at: FPCoordinate,
    queue: BinaryHeap<Nearer>,
}

impl Iterator for Browsing<'_> {
    type Item = Found;

    fn next(&mut self) -> Option<Found> {
        while let Some(Nearer { away, what }) = self.queue.pop() {
            match what {
                // a thing off the top of the queue is nearer than everything
                // still on it, boxes included, so it is the next answer
                Step::Thing(at, lat, lon) => {
                    let item = self.over.item_at(at);
                    return Some(Found {
                        what: item.what,
                        at: FPCoordinate::new(lat, lon),
                        away: away * METRES_A_DEGREE / FIXED,
                    });
                }
                Step::Boxes(level, which) => {
                    if level == 0 {
                        // the bottom row: what is under it is the things
                        let first = which * self.over.fan;
                        let upto = (first + self.over.fan).min(self.over.objects);
                        for at in first..upto {
                            let item = self.over.item_at(at);
                            let (away, near) =
                                scaled_item_distance(&item, self.at, self.over.lon_scale);
                            self.queue.push(Nearer {
                                away,
                                what: Step::Thing(
                                    at,
                                    near.y as i32,
                                    (near.x / self.over.lon_scale) as i32,
                                ),
                            });
                        }
                    } else {
                        let first = which * self.over.fan;
                        let upto =
                            (first + self.over.fan).min(self.over.counts[level - 1] as usize);
                        for child in first..upto {
                            let held = self.over.box_at(level - 1, child);
                            self.queue.push(Nearer {
                                away: scaled_min_distance(&held, self.at, self.over.lon_scale),
                                what: Step::Boxes(level - 1, child),
                            });
                        }
                    }
                }
            }
        }
        None
    }
}

/// Puts things in Sort-Tile-Recursive order: sorted by one axis into slabs,
/// and each slab sorted by the other.
///
/// A box built over a run of things in this order holds things that are near
/// each other, which is what makes its floor tight enough to be worth having.
fn sort_tile_recursive(items: &mut [Item], lon_scale: f64) {
    if items.len() <= FAN {
        return;
    }
    let middle = |item: &Item| {
        (
            f64::from(item.from.lon + item.to.lon) / 2.0 * lon_scale,
            f64::from(item.from.lat + item.to.lat) / 2.0,
        )
    };
    items.sort_by(|a, b| {
        middle(a)
            .0
            .partial_cmp(&middle(b).0)
            .unwrap_or(Ordering::Equal)
    });
    let pages = items.len().div_ceil(FAN);
    let slabs = (pages as f64).sqrt().ceil() as usize;
    let apiece = items.len().div_ceil(slabs.max(1));
    for slab in items.chunks_mut(apiece.max(1)) {
        slab.sort_by(|a, b| {
            middle(a)
                .1
                .partial_cmp(&middle(b).1)
                .unwrap_or(Ordering::Equal)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{edge::InputEdge, static_graph::StaticGraph};

    /// Places spread over a patch of Europe, at a spacing a road network has.
    fn places(side: usize) -> Vec<FPCoordinate> {
        let mut held = Vec::new();
        for row in 0..side {
            for column in 0..side {
                held.push(FPCoordinate::new(
                    48_000_000 + (row as i32) * 7_919,
                    9_000_000 + (column as i32) * 6_733,
                ));
            }
        }
        held
    }

    fn grid_graph(side: usize) -> StaticGraph<u32> {
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, 1_u32));
                    edges.push(InputEdge::new(node + 1, node, 1_u32));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, 1_u32));
                    edges.push(InputEdge::new(node + side, node, 1_u32));
                }
            }
        }
        StaticGraph::new(edges)
    }

    /// The answer worked out by asking every thing, which is what the tree has
    /// to agree with.
    fn by_hand(index: &NearestIndex, items: &[Item], at: FPCoordinate) -> f64 {
        items
            .iter()
            .map(|item| scaled_item_distance(item, at, index.lon_scale).0)
            .fold(f64::INFINITY, f64::min)
    }

    fn items_of(index: &NearestIndex) -> Vec<Item> {
        (0..index.len()).map(|at| index.item_at(at)).collect()
    }

    /// Somewhere to ask about, spread over the data and beyond its edges.
    fn asked(which: usize) -> FPCoordinate {
        let step = which as i32;
        FPCoordinate::new(
            47_800_000 + (step * 39_301) % 900_000,
            8_800_000 + (step * 27_733) % 900_000,
        )
    }

    /// The one that matters: the nearest node the tree finds is the nearest
    /// node there is.
    #[test]
    fn the_nearest_node_is_the_nearest_node() {
        let index = NearestIndex::over_nodes(&places(40));
        let items = items_of(&index);
        assert_eq!(index.kind(), Kind::Nodes);
        assert_eq!(index.len(), 1600);

        for which in 0..200 {
            let at = asked(which);
            let found = index.nearest(at).expect("a node");
            let wanted = by_hand(&index, &items, at);
            let away = found.away * FIXED / METRES_A_DEGREE;
            assert!(
                (away - wanted).abs() < 1e-6,
                "asked {which}: found {away}, nearest is {wanted}"
            );
            // and what it says it found is where it says it is
            assert_eq!(found.at, places(40)[found.what as usize]);
        }
    }

    /// And the nearest piece of road is the nearest piece of road, which is
    /// not the same question: the near point is usually not an end of it.
    #[test]
    fn the_nearest_segment_is_the_nearest_segment() {
        let side = 40;
        let index = NearestIndex::over_segments(&grid_graph(side), &places(side));
        let items = items_of(&index);
        assert_eq!(index.kind(), Kind::Segments);
        // one segment a road, not one an arc
        assert_eq!(index.len(), 2 * side * (side - 1));

        let mut between_the_ends = 0;
        for which in 0..200 {
            let at = asked(which);
            let found = index.nearest(at).expect("a segment");
            let wanted = by_hand(&index, &items, at);
            let away = found.away * FIXED / METRES_A_DEGREE;
            assert!(
                (away - wanted).abs() < 1e-6,
                "asked {which}: found {away}, nearest is {wanted}"
            );
            let item = items
                .iter()
                .find(|item| item.what == found.what)
                .expect("the item");
            if found.at != item.from && found.at != item.to {
                between_the_ends += 1;
            }
        }
        assert!(
            between_the_ends > 100,
            "only {between_the_ends} of two hundred fell between the ends: this is \
             answering as though it were a node index"
        );
    }

    /// Browsing hands them out nearest first and hands out every one of them.
    #[test]
    fn browsing_hands_them_out_in_order_and_leaves_none_behind() {
        let index = NearestIndex::over_nodes(&places(20));
        for which in 0..20 {
            let at = asked(which);
            let mut last = f64::NEG_INFINITY;
            let mut seen = std::collections::HashSet::new();
            for found in index.browse(at) {
                assert!(
                    found.away >= last - 1e-9,
                    "handed out {} after {last}",
                    found.away
                );
                last = found.away;
                assert!(seen.insert(found.what), "handed out {} twice", found.what);
            }
            assert_eq!(seen.len(), index.len(), "some were never handed out");
        }
    }

    /// A query inside the data and one far outside it both answer, and the far
    /// one answers with something on the edge nearest it.
    #[test]
    fn a_query_outside_the_data_still_finds_the_nearest_thing() {
        let index = NearestIndex::over_nodes(&places(16));
        let items = items_of(&index);
        for at in [
            FPCoordinate::new(0, 0),
            FPCoordinate::new(80_000_000, 170_000_000),
            FPCoordinate::new(48_050_000, 9_050_000),
        ] {
            let found = index.nearest(at).expect("a node");
            let wanted = by_hand(&index, &items, at);
            assert!((found.away * FIXED / METRES_A_DEGREE - wanted).abs() < 1e-6);
        }
    }

    /// The one that says the two variants are one implementation: an index
    /// read off a file answers what the one it was written from does.
    #[test]
    fn an_index_read_off_a_file_answers_what_the_one_it_came_from_does() {
        let side = 32;
        let held = tempfile::tempdir().expect("a directory to write in");
        for (name, whole) in [
            ("nodes", NearestIndex::over_nodes(&places(side))),
            (
                "segments",
                NearestIndex::over_segments(&grid_graph(side), &places(side)),
            ),
        ] {
            let path = held.path().join(name);
            whole.save(&path).expect("an index to write");
            // small enough that it is reading and letting go throughout
            let pool = Pool::of(2 * crate::paged_array::BLOCK_BYTES);
            let read = NearestIndex::open(&path, &pool).expect("an index to read");

            assert_eq!(read.kind(), whole.kind());
            assert_eq!(read.len(), whole.len());
            assert_eq!(read.levels(), whole.levels());
            for which in 0..120 {
                let at = asked(which);
                assert_eq!(
                    read.nearest(at),
                    whole.nearest(at),
                    "asked {which} of {name}"
                );
            }
            // and the whole order, not only the first of it
            let at = asked(7);
            let one: Vec<_> = whole.browse(at).take(50).collect();
            let other: Vec<_> = read.browse(at).take(50).collect();
            assert_eq!(one, other, "the order differs for {name}");
        }
    }

    /// What it costs standing still goes with the levels and not the things.
    #[test]
    fn an_index_read_off_a_file_costs_the_same_however_much_it_holds() {
        let held = tempfile::tempdir().expect("a directory to write in");
        let pool = Pool::of(1 << 20);
        let mut sizes = Vec::new();
        for side in [16_usize, 64] {
            let path = held.path().join(format!("index{side}"));
            NearestIndex::over_nodes(&places(side))
                .save(&path)
                .expect("an index to write");
            let read = NearestIndex::open(&path, &pool).expect("an index to read");
            sizes.push((read.len(), read.bytes()));
        }
        assert!(sizes[1].0 > sizes[0].0 * 8, "the two are the same size");
        for &(things, bytes) in &sizes {
            assert!(
                bytes < 1024,
                "an index over {things} things costs {bytes} bytes standing still"
            );
        }
    }

    #[test]
    fn an_index_that_is_read_cannot_be_written_back() {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("index");
        NearestIndex::over_nodes(&places(8))
            .save(&path)
            .expect("an index to write");
        let read = NearestIndex::open(&path, &Pool::of(1 << 20)).expect("an index to read");
        assert!(read.save(&held.path().join("again")).is_err());
    }

    /// A box's key has to be a floor on everything under it, or the walk hands
    /// out a far thing before a near one.
    #[test]
    fn a_box_is_never_further_than_what_is_under_it() {
        let side = 24;
        let index = NearestIndex::over_segments(&grid_graph(side), &places(side));
        for which in 0..40 {
            let at = asked(which);
            for level in 0..index.levels() {
                for node in 0..index.counts[level] as usize {
                    let floor =
                        scaled_min_distance(&index.box_at(level, node), at, index.lon_scale);
                    // everything under this box, walked down to the things
                    let mut wide = vec![node];
                    for below in (0..level).rev() {
                        wide = wide
                            .iter()
                            .flat_map(|&which| {
                                (which * FAN..(which + 1) * FAN)
                                    .take_while(|&at| at < index.counts[below] as usize)
                            })
                            .collect();
                    }
                    for &page in &wide {
                        for at_item in page * FAN..((page + 1) * FAN).min(index.len()) {
                            let away =
                                scaled_item_distance(&index.item_at(at_item), at, index.lon_scale)
                                    .0;
                            assert!(
                                floor <= away + 1e-9,
                                "a box at level {level} says {floor} and holds {away}"
                            );
                        }
                    }
                }
            }
        }
    }
}
