//! The nearest node, or the nearest piece of road, to a place on the map.
//!
//! # What became of the other R-tree
//!
//! There was a second one here, `r_tree`, which browsed by distance over a
//! tree of `Vec`s. It could not page, which is the thing an offline instance
//! most needs, and it was used by nothing. What it had that this did not --
//! asking what lies in a patch of the map rather than what is nearest a point
//! -- is [`intersecting`](NearestIndex::intersecting), and what it had that
//! was worth more, the [`Metric`](crate::metric::Metric) abstraction, this now
//! measures through. There is one tree here.
//!
//! That abstraction recorded a mismatch it could not fix: its great-circle
//! bound on a box is a flat rectangle's, and can read too long, which is
//! exactly the contract a browse rests on. [`Scaled`] is the measure that does
//! satisfy it, and is what this uses.
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
    block_codec::Codec,
    block_map::BlockMap,
    block_store::{BlockWriter, read_at},
    bounding_box::BoundingBox,
    geometry::FPCoordinate,
    graph::Arcs,
    metric::{Metric, Scaled},
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

/// Something an index can hold.
///
/// # What a shape has to say about itself
///
/// Where it is, so it can be put in an order that makes the boxes over it
/// tight; what box holds it, so a walk can bound it; how far away it is and
/// what point of it is nearest, so a walk can rank it against the boxes; and
/// how to write it down and read it back, at a width that does not vary, so
/// that where it sits in a file is arithmetic and the index can be read a
/// block at a time.
///
/// The fixed width is the only demand that is not obvious, and it is the one
/// that buys the paging. A shape with a variable-length tail keeps the tail
/// somewhere else and holds an offset here.
///
/// # The contract a walk rests on
///
/// [`nearest_to`](Self::nearest_to) must never answer less than
/// [`Metric::min_distance`] of [`bbox`](Self::bbox), in the same measure. A
/// walk keys boxes on the bound and shapes on this, so a shape that reports
/// nearer than the box around it is a shape handed out before things that are
/// closer.
pub trait Indexed: Copy {
    /// How wide one is written, in bytes.
    const BYTES: usize;

    /// What kind of thing this is, so a file says what is in it and reading it
    /// as something else is refused rather than answered wrongly.
    const TAG: u32;

    /// Writes one down. `into` is at least [`BYTES`](Self::BYTES) long.
    fn write_to(&self, into: &mut [u8]);

    /// Reads one back.
    fn read_from(from: &[u8]) -> Self;

    /// The box that holds it.
    fn bbox(&self) -> BoundingBox;

    /// Where it sits, for the sort that decides which things share a box.
    fn center(&self) -> FPCoordinate;

    /// How far away it is, and what point of it is nearest.
    fn nearest_to(&self, at: &FPCoordinate, by: &Scaled) -> (f64, FPCoordinate);
}

/// A node of the graph, at a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub what: u32,
    pub at: FPCoordinate,
}

impl Indexed for Node {
    const BYTES: usize = 4 + 8;
    const TAG: u32 = 1;

    fn write_to(&self, into: &mut [u8]) {
        into[0..4].copy_from_slice(&self.what.to_le_bytes());
        into[4..8].copy_from_slice(&self.at.lat.to_le_bytes());
        into[8..12].copy_from_slice(&self.at.lon.to_le_bytes());
    }

    fn read_from(from: &[u8]) -> Self {
        Self {
            what: u32::from_le_bytes(from[0..4].try_into().expect("four bytes")),
            at: FPCoordinate::new(word(&from[4..8]), word(&from[8..12])),
        }
    }

    fn bbox(&self) -> BoundingBox {
        BoundingBox::between(self.at, self.at)
    }

    fn center(&self) -> FPCoordinate {
        self.at
    }

    fn nearest_to(&self, at: &FPCoordinate, by: &Scaled) -> (f64, FPCoordinate) {
        (by.distance(at, &self.at), self.at)
    }
}

/// A piece of road, between the ends of an arc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub what: u32,
    pub from: FPCoordinate,
    pub to: FPCoordinate,
}

impl Indexed for Segment {
    const BYTES: usize = 4 + 8 + 8;
    const TAG: u32 = 2;

    fn write_to(&self, into: &mut [u8]) {
        into[0..4].copy_from_slice(&self.what.to_le_bytes());
        into[4..8].copy_from_slice(&self.from.lat.to_le_bytes());
        into[8..12].copy_from_slice(&self.from.lon.to_le_bytes());
        into[12..16].copy_from_slice(&self.to.lat.to_le_bytes());
        into[16..20].copy_from_slice(&self.to.lon.to_le_bytes());
    }

    fn read_from(from: &[u8]) -> Self {
        Self {
            what: u32::from_le_bytes(from[0..4].try_into().expect("four bytes")),
            from: FPCoordinate::new(word(&from[4..8]), word(&from[8..12])),
            to: FPCoordinate::new(word(&from[12..16]), word(&from[16..20])),
        }
    }

    fn bbox(&self) -> BoundingBox {
        BoundingBox::from_coordinates(&[self.from, self.to])
    }

    fn center(&self) -> FPCoordinate {
        FPCoordinate::new(
            (self.from.lat + self.to.lat) / 2,
            (self.from.lon + self.to.lon) / 2,
        )
    }

    fn nearest_to(&self, at: &FPCoordinate, by: &Scaled) -> (f64, FPCoordinate) {
        by.distance_to_segment(at, &self.from, &self.to)
    }
}

/// The two an index over a road network is built from.
///
/// A caller indexing anything else writes its own [`Indexed`] and hands the
/// shapes to [`NearestIndex::over`]; these are here because a router asks for
/// them constantly and because building them off a graph is the same three
/// lines every time.
impl NearestIndex<Node> {
    /// An index over the nodes of a graph.
    ///
    /// # Panics
    ///
    /// Panics for more nodes than four thousand million.
    #[must_use]
    pub fn over_nodes(coordinates: &[FPCoordinate]) -> Self {
        Self::over(
            coordinates
                .iter()
                .enumerate()
                .map(|(at, &place)| Node {
                    what: u32::try_from(at).expect("a node in four bytes"),
                    at: place,
                })
                .collect(),
        )
    }
}

impl NearestIndex<Segment> {
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
                items.push(Segment {
                    what: u32::try_from(edge).expect("an arc in four bytes"),
                    from: coordinates[node],
                    to: coordinates[target],
                });
            }
        }
        Self::over(items)
    }
}

/// How wide a [`BoundingBox`] is written.
const BOX_BYTES: usize = 16;

/// What a browse hands back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found<T> {
    /// the thing itself
    pub what: T,
    /// what point of it is nearest
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
enum Items<T> {
    Held(Vec<T>),
    Paged(PagedArray),
}

/// What a written index says about itself before its arrays.
#[derive(Archive, Serialize, Deserialize)]
struct Head {
    version: u16,
    /// what shape the things are, so reading them as another is refused
    tag: u32,
    fan: u32,
    objects: u64,
    /// how many boxes each level has, the bottom row first
    counts: Vec<u32>,
    lon_scale: f64,
    /// how many entries a block of each holds, and where every block is
    apiece: u32,
    boxes: BlockMap,
    items: BlockMap,
}

/// How many entries go in a block of an index.
///
/// Enough that the block is worth a codec's while and few enough that reading
/// one to answer a query is not reading a great deal that will not be looked
/// at. Sixty four kibibytes of boxes is four thousand of them.
pub const IN_A_BLOCK: usize = 4096;

/// What the two runs of a written index are filed under.
const BOXES: u8 = u8::MAX - 2;
const THINGS: u8 = u8::MAX - 3;

/// A nearest-first index over the nodes or the arcs of a graph.
pub struct NearestIndex<T: Indexed> {
    fan: usize,
    objects: usize,
    /// where each level begins in the flat run of boxes, the bottom row first
    level_at: Vec<u64>,
    counts: Vec<u32>,
    boxes: Boxes,
    items: Items<T>,
    /// the plane everything is measured in
    plane: Scaled,
}

impl<T: Indexed> NearestIndex<T> {
    /// An index over whatever shapes a caller has.
    ///
    /// # Panics
    ///
    /// Panics for nothing to index, which has no box to be measured in.
    #[must_use]
    pub fn over(items: Vec<T>) -> Self {
        Self::of(items)
    }

    /// Builds the tree over things already collected.
    fn of(mut items: Vec<T>) -> Self {
        let mut whole = BoundingBox::invalid();
        for item in &items {
            whole.extend_with(&item.bbox());
        }
        let plane = Scaled::about(&whole);

        sort_tile_recursive(&mut items, plane.lon_scale);

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
                held.extend_with(&item.bbox());
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
            fan: FAN,
            objects: items.len(),
            level_at,
            counts,
            boxes: Boxes::Held(boxes),
            items: Items::Held(items),
            plane,
        }
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
                Items::Held(held) => held.capacity() * size_of::<T>(),
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
    fn item_at(&self, at: usize) -> T {
        match &self.items {
            Items::Held(held) => held[at],
            Items::Paged(read) => {
                // a shape says how wide it is at run time, so the buffer is
                // sized here rather than named in a turbofish
                let mut into = [0_u8; 64];
                assert!(
                    T::BYTES <= into.len(),
                    "a shape wider than sixty four bytes"
                );
                assert!(read.get_into(at, &mut into), "a thing the index has");
                T::read_from(&into)
            }
        }
    }

    /// Every thing there is, nearest first.
    ///
    /// The walk is lazy: it does no more work than the caller takes answers,
    /// so asking for the first is not asking for a sort.
    #[must_use]
    pub fn browse(&self, at: FPCoordinate) -> Browsing<'_, T> {
        let mut queue = BinaryHeap::new();
        if self.objects > 0 {
            let top = self.counts.len() - 1;
            for which in 0..self.counts[top] as usize {
                let box_at = self.box_at(top, which);
                queue.push(Nearer {
                    away: self.plane.min_distance(&box_at, &at),
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

    /// Every thing whose own box meets the given one, in no particular order.
    ///
    /// This is the other question a tree of boxes answers, and the one
    /// [`RTree`](crate::r_tree::RTree) called `intersecting`: what lies in
    /// this patch of the map, rather than what is nearest to this point. It
    /// walks the same implicit tree, keeping whatever meets the box and
    /// dropping whole subtrees that do not.
    ///
    /// A thing is handed out when the box round it meets the query, which for
    /// a segment is the box round its two ends: a road may be kept whose line
    /// misses the query even though the box round it does not.
    #[must_use]
    pub fn intersecting(&self, within: BoundingBox) -> Intersecting<'_, T> {
        let mut stack = Vec::new();
        if self.objects > 0 {
            let top = self.counts.len() - 1;
            for which in (0..self.counts[top] as usize).rev() {
                stack.push((top, which));
            }
        }
        Intersecting {
            over: self,
            within,
            stack,
            things: Vec::new(),
        }
    }

    /// The nearest thing, and nothing where the index is empty.
    #[must_use]
    pub fn nearest(&self, at: FPCoordinate) -> Option<Found<T>> {
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
        // The two runs go through BlockWriter, which is what the cell tables
        // and the arc blocks go through: the same codec, the same entry, the
        // same map out the other end. An index over a continent's roads is
        // three quarters of a gibibyte written plainly, which is far too much
        // to leave to a codec that was never run.
        let beside = blocks_beside(path);
        let mut writer = BlockWriter::create(&beside)?;
        let boxes_map = write_run(&mut writer, BOXES, boxes.len(), BOX_BYTES, |at, into| {
            let (min, max) = boxes[at].corners();
            into.extend_from_slice(&min.lat.to_le_bytes());
            into.extend_from_slice(&min.lon.to_le_bytes());
            into.extend_from_slice(&max.lat.to_le_bytes());
            into.extend_from_slice(&max.lon.to_le_bytes());
        })?;
        let mut one = vec![0_u8; T::BYTES];
        let items_map = write_run(&mut writer, THINGS, items.len(), T::BYTES, |at, into| {
            items[at].write_to(&mut one);
            into.extend_from_slice(&one);
        })?;
        let both = writer.finish()?;
        debug_assert_eq!(both.len(), boxes_map.len() + items_map.len());

        let head = Head {
            version: VERSION,
            tag: T::TAG,
            fan: u32::try_from(self.fan).expect("a fan in four bytes"),
            objects: self.objects as u64,
            counts: self.counts.clone(),
            lon_scale: self.plane.lon_scale,
            apiece: u32::try_from(IN_A_BLOCK).expect("a block in four bytes"),
            boxes: boxes_map,
            items: items_map,
        };
        let head = rkyv::to_bytes::<rkyv::rancor::Error>(&head)
            .map_err(|why| std::io::Error::other(format!("an index will not serialize: {why}")))?;
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&(head.len() as u64).to_le_bytes())?;
        out.write_all(&head)?;
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
        if head.tag != T::TAG {
            return Err(std::io::Error::other(format!(
                "an index of shape {} read as shape {}",
                head.tag,
                T::TAG
            )));
        }

        let mut level_at = Vec::with_capacity(head.counts.len());
        let mut running = 0_u64;
        for &count in &head.counts {
            level_at.push(running);
            running += u64::from(count);
        }
        let boxes = running;

        let beside = blocks_beside(path);
        let apiece = head.apiece as usize;
        Ok(Self {
            fan: head.fan as usize,
            objects: head.objects as usize,
            level_at,
            counts: head.counts,
            boxes: Boxes::Paged(PagedArray::open_blocks(
                &beside,
                head.boxes,
                boxes as usize,
                BOX_BYTES,
                apiece,
                Arc::clone(pool),
            )?),
            items: Items::Paged(PagedArray::open_blocks(
                &beside,
                head.items,
                head.objects as usize,
                T::BYTES,
                apiece,
                Arc::clone(pool),
            )?),
            plane: Scaled {
                lon_scale: head.lon_scale,
            },
        })
    }
}

/// Where the blocks of an index go, beside the head that names them.
///
/// Appended rather than put in place of an extension the caller chose: two
/// indexes written as `held.nodes` and `held.segments` would otherwise both
/// want `held.blocks`, and the second would quietly write over the first.
#[must_use]
pub fn blocks_beside(path: &Path) -> std::path::PathBuf {
    let mut beside = path.as_os_str().to_os_string();
    beside.push(".blocks");
    beside.into()
}

/// Writes a run of fixed-width entries as blocks of [`IN_A_BLOCK`] apiece.
///
/// A block holds a fixed count and not a fixed size, so which block an entry
/// is in stays arithmetic and only how long the block turned out to be has to
/// be written down.
fn write_run(
    out: &mut BlockWriter,
    tag: u8,
    entries: usize,
    wide: usize,
    mut one: impl FnMut(usize, &mut Vec<u8>),
) -> std::io::Result<BlockMap> {
    let mut into = Vec::new();
    for block in 0..entries.div_ceil(IN_A_BLOCK) {
        let first = block * IN_A_BLOCK;
        let upto = (first + IN_A_BLOCK).min(entries);
        into.clear();
        for at in first..upto {
            one(at, &mut into);
        }
        // by column, so the codec sees the high bytes of a coordinate as one
        // run rather than interleaved with the low ones
        let planed = crate::paged_array::for_writing(&into, wide);
        out.push_bytes(
            &planed,
            tag,
            (first as u128, (upto - 1) as u128),
            (u32::try_from(block).expect("a block in four bytes"), 1),
            (
                u32::try_from(first).expect("an entry in four bytes"),
                u32::try_from(upto - first).expect("a run in four bytes"),
            ),
            Codec::Lz4,
            3,
        )?;
    }
    // the writer holds every block of the file, and what is wanted here is the
    // ones this run put in it
    Ok(BlockMap::of(
        out.written()
            .iter()
            .filter(|entry| entry.level == tag)
            .copied()
            .collect(),
    ))
}

/// Four bytes of a written coordinate.
fn word(held: &[u8]) -> i32 {
    i32::from_le_bytes(held.try_into().expect("four bytes"))
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
pub struct Browsing<'a, T: Indexed> {
    over: &'a NearestIndex<T>,
    at: FPCoordinate,
    queue: BinaryHeap<Nearer>,
}

impl<T: Indexed> Iterator for Browsing<'_, T> {
    type Item = Found<T>;

    fn next(&mut self) -> Option<Found<T>> {
        while let Some(Nearer { away, what }) = self.queue.pop() {
            match what {
                // a thing off the top of the queue is nearer than everything
                // still on it, boxes included, so it is the next answer
                Step::Thing(at, lat, lon) => {
                    let item = self.over.item_at(at);
                    return Some(Found {
                        what: item,
                        at: FPCoordinate::new(lat, lon),
                        away: self.over.plane.metres(away),
                    });
                }
                Step::Boxes(level, which) => {
                    if level == 0 {
                        // the bottom row: what is under it is the things
                        let first = which * self.over.fan;
                        let upto = (first + self.over.fan).min(self.over.objects);
                        for at in first..upto {
                            let item = self.over.item_at(at);
                            let (away, near) = item.nearest_to(&self.at, &self.over.plane);
                            self.queue.push(Nearer {
                                away,
                                what: Step::Thing(at, near.lat, near.lon),
                            });
                        }
                    } else {
                        let first = which * self.over.fan;
                        let upto =
                            (first + self.over.fan).min(self.over.counts[level - 1] as usize);
                        for child in first..upto {
                            let held = self.over.box_at(level - 1, child);
                            self.queue.push(Nearer {
                                away: self.over.plane.min_distance(&held, &self.at),
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
fn sort_tile_recursive<T: Indexed>(items: &mut [T], lon_scale: f64) {
    if items.len() <= FAN {
        return;
    }
    let middle = |item: &T| {
        let at = item.center();
        (f64::from(at.lon) * lon_scale, f64::from(at.lat))
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
pub(crate) mod tests {
    use super::*;
    use crate::{edge::InputEdge, static_graph::StaticGraph};

    /// Places spread over a patch of Europe, at a spacing a road network has.
    pub(crate) fn places(side: usize) -> Vec<FPCoordinate> {
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

    pub(crate) fn grid_graph(side: usize) -> StaticGraph<u32> {
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
    fn by_hand<T: Indexed>(index: &NearestIndex<T>, items: &[T], at: FPCoordinate) -> f64 {
        items
            .iter()
            .map(|item| item.nearest_to(&at, &index.plane).0)
            .fold(f64::INFINITY, f64::min)
    }

    pub(crate) fn items_of<T: Indexed>(index: &NearestIndex<T>) -> Vec<T> {
        (0..index.len()).map(|at| index.item_at(at)).collect()
    }

    /// Somewhere to ask about, spread over the data and beyond its edges.
    pub(crate) fn asked(which: usize) -> FPCoordinate {
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
        assert_eq!(index.len(), 1600);

        for which in 0..200 {
            let at = asked(which);
            let found = index.nearest(at).expect("a node");
            let wanted = by_hand(&index, &items, at);
            let away = found.away / index.plane.metres(1.0);
            assert!(
                (away - wanted).abs() < 1e-6,
                "asked {which}: found {away}, nearest is {wanted}"
            );
            // and what it says it found is where it says it is
            assert_eq!(found.at, places(40)[found.what.what as usize]);
        }
    }

    /// And the nearest piece of road is the nearest piece of road, which is
    /// not the same question: the near point is usually not an end of it.
    #[test]
    fn the_nearest_segment_is_the_nearest_segment() {
        let side = 40;
        let index = NearestIndex::over_segments(&grid_graph(side), &places(side));
        let items = items_of(&index);
        // one segment a road, not one an arc
        assert_eq!(index.len(), 2 * side * (side - 1));

        let mut between_the_ends = 0;
        for which in 0..200 {
            let at = asked(which);
            let found = index.nearest(at).expect("a segment");
            let wanted = by_hand(&index, &items, at);
            let away = found.away / index.plane.metres(1.0);
            assert!(
                (away - wanted).abs() < 1e-6,
                "asked {which}: found {away}, nearest is {wanted}"
            );
            let item = items
                .iter()
                .find(|item| item.what == found.what.what)
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
                assert!(
                    seen.insert(found.what.what),
                    "handed out {} twice",
                    found.what.what
                );
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
            assert!((found.away / index.plane.metres(1.0) - wanted).abs() < 1e-6);
        }
    }

    /// The one that says the two variants are one implementation: an index
    /// read off a file answers what the one it was written from does.
    #[test]
    fn an_index_read_off_a_file_answers_what_the_one_it_came_from_does() {
        let side = 32;
        let held = tempfile::tempdir().expect("a directory to write in");
        both_ways(
            &held.path().join("nodes"),
            "nodes",
            NearestIndex::over_nodes(&places(side)),
        );
        both_ways(
            &held.path().join("segments"),
            "segments",
            NearestIndex::over_segments(&grid_graph(side), &places(side)),
        );
    }

    /// Written, read back, and asked the same questions as the one it came
    /// from -- for whatever shape it is over.
    fn both_ways<T: Indexed + std::fmt::Debug + PartialEq>(
        path: &std::path::Path,
        name: &str,
        whole: NearestIndex<T>,
    ) {
        whole.save(path).expect("an index to write");
        // small enough that it is reading and letting go throughout
        let pool = Pool::of(2 * crate::paged_array::BLOCK_BYTES);
        let read: NearestIndex<T> = NearestIndex::open(path, &pool).expect("an index to read");

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
            let read: NearestIndex<Node> =
                NearestIndex::open(&path, &pool).expect("an index to read");
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
        let read: NearestIndex<Node> =
            NearestIndex::open(&path, &Pool::of(1 << 20)).expect("an index to read");
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
                    let floor = index.plane.min_distance(&index.box_at(level, node), &at);
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
                            let item = index.item_at(at_item);
                            let away = index.plane.distance_to_segment(&at, &item.from, &item.to).0;
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

/// Every thing of an index whose box meets a given one.
pub struct Intersecting<'a, T: Indexed> {
    over: &'a NearestIndex<T>,
    within: BoundingBox,
    /// boxes still to look at, as a row and a place in it
    stack: Vec<(usize, usize)>,
    /// things found under the box last opened, still to hand out
    things: Vec<T>,
}

impl<T: Indexed> Iterator for Intersecting<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        loop {
            if let Some(item) = self.things.pop() {
                return Some(item);
            }
            let (level, which) = self.stack.pop()?;
            if !self.over.box_at(level, which).intersects(&self.within) {
                continue;
            }
            if level == 0 {
                let first = which * self.over.fan;
                let upto = (first + self.over.fan).min(self.over.objects);
                for at in first..upto {
                    let item = self.over.item_at(at);
                    if item.bbox().intersects(&self.within) {
                        self.things.push(item);
                    }
                }
            } else {
                let first = which * self.over.fan;
                let upto = (first + self.over.fan).min(self.over.counts[level - 1] as usize);
                for child in first..upto {
                    self.stack.push((level - 1, child));
                }
            }
        }
    }
}

/// What the tree answers besides "what is nearest": the box query the other
/// R-tree had, and the measure that makes both of them right.
#[cfg(test)]
mod asking {
    use super::{tests::*, *};

    /// Everything in a patch of the map, and nothing outside it.
    #[test]
    fn asking_what_lies_in_a_box_finds_what_lies_in_it() {
        let side = 32;
        let index = NearestIndex::over_nodes(&places(side));
        let items = items_of(&index);

        for (low, high) in [
            ((48_050_000, 9_050_000), (48_150_000, 9_150_000)),
            ((0, 0), (90_000_000, 90_000_000)),
            ((47_000_000, 8_000_000), (47_100_000, 8_100_000)),
        ] {
            let within = BoundingBox::from_coordinates(&[
                FPCoordinate::new(low.0, low.1),
                FPCoordinate::new(high.0, high.1),
            ]);
            let mut found: Vec<u32> = index.intersecting(within).map(|item| item.what).collect();
            found.sort_unstable();
            let mut wanted: Vec<u32> = items
                .iter()
                .filter(|item| within.contains(&item.at))
                .map(|item| item.what)
                .collect();
            wanted.sort_unstable();
            assert_eq!(found, wanted, "the box {low:?}..{high:?}");
        }
    }

    /// And it answers the same read off a file as held.
    #[test]
    fn a_box_query_answers_the_same_off_a_file() {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("index");
        let whole = NearestIndex::over_segments(&grid_graph(24), &places(24));
        whole.save(&path).expect("an index to write");
        let read: NearestIndex<Segment> =
            NearestIndex::open(&path, &Pool::of(2 * crate::paged_array::BLOCK_BYTES))
                .expect("an index to read");

        let within = BoundingBox::from_coordinates(&[
            FPCoordinate::new(48_030_000, 9_030_000),
            FPCoordinate::new(48_090_000, 9_090_000),
        ]);
        let mut one: Vec<u32> = whole.intersecting(within).map(|item| item.what).collect();
        let mut other: Vec<u32> = read.intersecting(within).map(|item| item.what).collect();
        one.sort_unstable();
        other.sort_unstable();
        assert!(!one.is_empty(), "the box holds nothing to compare");
        assert_eq!(one, other);
    }

    /// The measure a browse rests on: the scaled plane's bound on a box is
    /// never longer than the distance to a point of that box.
    #[test]
    fn the_scaled_plane_bounds_a_box_from_below() {
        let plane = Scaled::about_latitude(48_000_000);
        let held = BoundingBox::from_coordinates(&[
            FPCoordinate::new(48_000_000, 9_000_000),
            FPCoordinate::new(48_500_000, 10_000_000),
        ]);
        for lat in [47_000_000, 48_200_000, 49_500_000] {
            for lon in [8_000_000, 9_500_000, 11_000_000] {
                let at = FPCoordinate::new(lat, lon);
                let bound = plane.min_distance(&held, &at);
                for corner_lat in [48_000_000, 48_250_000, 48_500_000] {
                    for corner_lon in [9_000_000, 9_500_000, 10_000_000] {
                        let inside = FPCoordinate::new(corner_lat, corner_lon);
                        assert!(
                            bound <= plane.distance(&at, &inside) + 1e-9,
                            "the bound {bound} is longer than a point of the box"
                        );
                    }
                }
            }
        }
    }
}

/// A shape the crate does not know about, indexed by the same tree.
///
/// This is the whole of what a caller has to write to index something new, and
/// it is here as a test because "it is generic" is a claim that is only worth
/// anything if something outside the two built-in shapes has been through it.
#[cfg(test)]
mod another_shape {
    use super::{tests::*, *};

    /// A place with a radius: a roadworks site, a charging station, a parking
    /// area. Its box is the square round it and its distance is measured from
    /// its edge, so a query inside it is nowhere at all.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Patch {
        what: u32,
        at: FPCoordinate,
        radius: u32,
    }

    impl Indexed for Patch {
        const BYTES: usize = 4 + 8 + 4;
        const TAG: u32 = 0x5041_5443;

        fn write_to(&self, into: &mut [u8]) {
            into[0..4].copy_from_slice(&self.what.to_le_bytes());
            into[4..8].copy_from_slice(&self.at.lat.to_le_bytes());
            into[8..12].copy_from_slice(&self.at.lon.to_le_bytes());
            into[12..16].copy_from_slice(&self.radius.to_le_bytes());
        }

        fn read_from(from: &[u8]) -> Self {
            Self {
                what: u32::from_le_bytes(from[0..4].try_into().expect("four bytes")),
                at: FPCoordinate::new(
                    i32::from_le_bytes(from[4..8].try_into().expect("four bytes")),
                    i32::from_le_bytes(from[8..12].try_into().expect("four bytes")),
                ),
                radius: u32::from_le_bytes(from[12..16].try_into().expect("four bytes")),
            }
        }

        fn bbox(&self) -> BoundingBox {
            let out = self.radius as i32;
            BoundingBox::from_coordinates(&[
                FPCoordinate::new(self.at.lat - out, self.at.lon - out),
                FPCoordinate::new(self.at.lat + out, self.at.lon + out),
            ])
        }

        fn center(&self) -> FPCoordinate {
            self.at
        }

        fn nearest_to(&self, at: &FPCoordinate, by: &Scaled) -> (f64, FPCoordinate) {
            // measured from the edge, and nothing at all from inside it
            let middle = by.distance(at, &self.at);
            let edge = by.distance(
                &self.at,
                &FPCoordinate::new(self.at.lat + self.radius as i32, self.at.lon),
            );
            ((middle - edge).max(0.0), self.at)
        }
    }

    fn patches(side: usize) -> Vec<Patch> {
        places(side)
            .into_iter()
            .enumerate()
            .map(|(at, place)| Patch {
                what: at as u32,
                at: place,
                radius: 1_000 + (at as u32 % 7) * 500,
            })
            .collect()
    }

    /// A shape written outside this module goes in, comes out, and is ranked
    /// by what it says about itself rather than by anything the tree assumed.
    #[test]
    fn a_shape_the_crate_does_not_know_about_is_indexed_the_same_way() {
        let index = NearestIndex::over(patches(24));
        let items = items_of(&index);
        assert_eq!(index.len(), 576);

        for which in 0..80 {
            let at = asked(which);
            let found = index.nearest(at).expect("a patch");
            let wanted = items
                .iter()
                .map(|patch| patch.nearest_to(&at, &index.plane).0)
                .fold(f64::INFINITY, f64::min);
            let away = found.away / index.plane.metres(1.0);
            assert!(
                (away - wanted).abs() < 1e-6,
                "asked {which}: found {away}, nearest is {wanted}"
            );
        }
    }

    /// And it pages like the others, with the same answers.
    #[test]
    fn a_shape_of_a_caller_s_own_reads_back_off_a_file() {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("patches");
        let whole = NearestIndex::over(patches(24));
        whole.save(&path).expect("an index to write");
        let read: NearestIndex<Patch> =
            NearestIndex::open(&path, &Pool::of(2 * crate::paged_array::BLOCK_BYTES))
                .expect("an index to read");
        for which in 0..80 {
            let at = asked(which);
            assert_eq!(read.nearest(at), whole.nearest(at), "asked {which}");
        }
    }

    /// A file says what shape is in it, so reading it as another is refused
    /// rather than answered with nonsense off a misread run of bytes.
    #[test]
    fn an_index_of_one_shape_is_not_read_as_another() {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("patches");
        NearestIndex::over(patches(8))
            .save(&path)
            .expect("an index to write");
        let pool = Pool::of(1 << 20);
        assert!(NearestIndex::<Node>::open(&path, &pool).is_err());
        assert!(NearestIndex::<Segment>::open(&path, &pool).is_err());
        assert!(NearestIndex::<Patch>::open(&path, &pool).is_ok());
    }
}
