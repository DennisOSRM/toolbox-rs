//! One cache, for everything an instance reads.
//!
//! # Why one and not four
//!
//! An instance that pages holds four kinds of thing it did not have to keep: a
//! block of arcs, a cell's table, the block a table was read out of, and a way
//! it has already put back. Given a budget apiece, each of them respects its
//! own number and together they respect nothing: three caches of a hundred
//! megabytes are three hundred megabytes, and a device with a budget for the
//! whole of an instance has no way to say so.
//!
//! So there is one, and what it holds is whatever was wanted most recently
//! whatever kind it is. That is also the right answer and not only the
//! measurable one: a query that is walking arcs is not reading tables, and one
//! that is putting a way back is doing neither, so the three want the room at
//! different moments and a fixed split gives each of them less than it needs
//! at the moment it needs it.
//!
//! # What is not in here
//!
//! The levels an instance holds outright. Those are read once when the store
//! opens and never let go of, so they are a footing rather than a cache, and
//! the budget is split between the two before this is given what is left.

use std::sync::{Arc, Mutex};

use crate::{
    cell_block::CellBlock, graph::NodeID, graph_block::HeldArcs, level_directory::CellId, lru::LRU,
    paged_overlay::HeldTable,
};

/// What names one thing in the pool.
///
/// The kinds cannot collide: two of them may hold the same numbers and mean
/// different things, and the kind is part of the name rather than a hope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// a block of arcs, by its place in the pack
    Arcs(u32),
    /// a cell's table, by the level and the cell
    Table(u8, CellId),
    /// the block a run of tables was read out of, by the level and its first cell
    Block(u8, CellId),
    /// a way already put back, by its ends and the level it crosses
    Way(u32, u32, u8),
    /// a block of a fixed-width array, by which array and which block
    Array(u16, u32),
}

/// What one thing in the pool is.
#[derive(Clone, Debug)]
pub enum Held {
    Arcs(Arc<HeldArcs>),
    Table(Arc<HeldTable>),
    Block(Arc<CellBlock>),
    Way(Arc<Vec<NodeID>>),
    /// a run of bytes read off a file, as a paged array holds them
    Bytes(Arc<Vec<u8>>),
}

impl Held {
    /// What it costs the budget.
    ///
    /// What the thing itself takes, and what the pool takes to keep it: a key,
    /// an entry in the list and an entry in the index. Left out, a pool of
    /// small things is a pool that overruns its budget by more than it holds.
    #[must_use]
    pub fn bytes(&self) -> usize {
        let held = match self {
            Self::Arcs(arcs) => arcs.bytes(),
            Self::Table(table) => table.bytes(),
            // `bytes` and not `framing_bytes`: the framing is what a block
            // costs beyond its entries, and the entries are nearly all of it.
            // Counted the other way a sixty four kibibyte block reads as a few
            // hundred bytes and the pool holds many times its budget.
            Self::Block(block) => block.bytes(),
            Self::Way(way) => way.capacity() * size_of::<NodeID>(),
            Self::Bytes(held) => held.capacity(),
        };
        held + size_of::<Key>() + size_of::<Self>() + PER_ENTRY
    }
}

/// What the list and the index take for an entry, beyond the entry itself.
const PER_ENTRY: usize = 64;

/// How the pool has been faring.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    pub hits: usize,
    pub misses: usize,
    pub evicted: usize,
    /// how many blocks were read off a file, of every kind: arcs, tables, the
    /// blocks tables come out of, and the fixed-width arrays. Everything that
    /// reads goes through the pool, so this is what an instance costs in reads
    /// however many structures it is made of.
    pub reads: usize,
    /// what the pool is holding, in bytes
    pub held: usize,
    /// the most it has ever held
    pub highest: usize,
}

/// How many buffers of each kind to keep for the next read.
///
/// A free list and not a cache: it holds nothing anybody wants, only room
/// somebody is about to want again. A few is enough, since what it is for is
/// the gap between letting one block go and reading the next.
const SPARE: usize = 16;

/// Buffers kept to be filled again.
///
/// # Why the room is kept when the contents are not
///
/// A block read builds three or four vectors, fills them, and gives them back
/// when the block is let go of. Over a run of a few hundred thousand reads
/// that is a million allocations of up to sixty four kibibytes apiece, and an
/// allocator handed that pattern fragments and does not give the pages back:
/// a pool that never holds more than its budget can still sit inside a process
/// several times that size.
///
/// So the vectors are not dropped. On the way out of the cache they are
/// emptied and put here, and the next read fills them again.
#[derive(Default)]
struct Scrap {
    arcs: Vec<HeldArcs>,
    tables: Vec<HeldTable>,
    bytes: Vec<Vec<u8>>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // what it is holding and what it may, and not the contents: a pool
        // printed entry by entry is a hundred thousand lines
        let faults = self.faults();
        f.debug_struct("Pool")
            .field("budget", &self.budget)
            .field("held", &faults.held)
            .finish()
    }
}

struct Inside {
    /// what may be let go of, oldest first
    kept: LRU<Key, Held>,
    /// What may not.
    ///
    /// The partition and the tree are asked something for every node a search
    /// settles and every cell it touches, and they are small: seven mebibytes
    /// of a hundred and twenty eight. Left to take their chances in the list
    /// they are pushed out by the arcs, which are read once and walked, and
    /// then read again for the next query -- so the hottest lookups in the
    /// engine pay a block read and the coldest keep the room.
    ///
    /// Held here they are still the budget's: what is pinned is counted, and
    /// a pool whose pins fill it has nothing left to cache with. It is the
    /// letting go that they are exempt from, not the accounting.
    stuck: rustc_hash::FxHashMap<Key, Held>,
    pinned: usize,
    bytes: usize,
    faults: Faults,
    scrap: Scrap,
}

/// A byte-budgeted cache shared by everything that reads.
pub struct Pool {
    budget: usize,
    inside: Mutex<Inside>,
}

impl Pool {
    /// A pool that may hold so many bytes.
    #[must_use]
    pub fn of(budget: usize) -> Arc<Self> {
        Arc::new(Self {
            budget,
            inside: Mutex::new(Inside {
                // room for the entries, which the budget in bytes bounds; the
                // count is only what the index is made large enough for
                kept: LRU::new_with_capacity(1 << 18),
                stuck: rustc_hash::FxHashMap::default(),
                pinned: 0,
                bytes: 0,
                faults: Faults::default(),
                scrap: Scrap::default(),
            }),
        })
    }

    /// What it may hold.
    #[must_use]
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// What it is holding, and how it has been faring.
    #[must_use]
    pub fn faults(&self) -> Faults {
        let inside = self.inside.lock().expect("the pool");
        Faults {
            held: inside.bytes + inside.pinned,
            ..inside.faults
        }
    }

    /// Whatever is under this name, where anything is.
    #[must_use]
    pub fn get(&self, key: &Key) -> Option<Held> {
        let mut inside = self.inside.lock().expect("the pool");
        if let Some(held) = inside.stuck.get(key) {
            let held = held.clone();
            inside.faults.hits += 1;
            return Some(held);
        }
        match inside.kept.get(key) {
            Some(held) => {
                let held = held.clone();
                inside.faults.hits += 1;
                Some(held)
            }
            None => {
                inside.faults.misses += 1;
                None
            }
        }
    }

    /// Puts something under a name, letting go of whatever it has to.
    ///
    /// Room is made before the thing is put down, so the budget bounds what is
    /// held rather than what was held a moment ago. A thing too large for the
    /// whole budget is not kept at all: it is handed back to the caller either
    /// way, and keeping it would empty the pool to hold one entry.
    pub fn put(&self, key: Key, held: Held) {
        let cost = held.bytes();
        if cost > self.budget {
            return;
        }
        let mut inside = self.inside.lock().expect("the pool");
        while inside.bytes + inside.pinned + cost > self.budget
            && let Some((_, gone)) = inside.kept.pop_lru()
        {
            inside.bytes -= gone.bytes();
            inside.faults.evicted += 1;
            recycle(&mut inside.scrap, gone);
        }
        // a pool whose pins leave no room keeps nothing else
        if inside.bytes + inside.pinned + cost <= self.budget {
            inside.kept.push(&key, held);
            inside.bytes += cost;
        }
        inside.faults.highest = inside.faults.highest.max(inside.bytes + inside.pinned);
    }

    /// Puts something in that will not be let go of.
    ///
    /// It counts against the budget like anything else; what it is exempt from
    /// is eviction. A caller that pins more than the budget gets a pool with
    /// nothing left to cache with, which is a caller's mistake and not the
    /// pool's, so it is allowed and reported rather than refused.
    pub fn pin(&self, key: Key, held: Held) {
        let cost = held.bytes();
        let mut inside = self.inside.lock().expect("the pool");
        // room first, out of what may be let go of
        while inside.bytes + inside.pinned + cost > self.budget
            && let Some((_, gone)) = inside.kept.pop_lru()
        {
            inside.bytes -= gone.bytes();
            inside.faults.evicted += 1;
            recycle(&mut inside.scrap, gone);
        }
        if inside.stuck.insert(key, held).is_none() {
            inside.pinned += cost;
        }
        let held = inside.bytes + inside.pinned;
        inside.faults.highest = inside.faults.highest.max(held);
    }

    /// What is pinned, in bytes.
    #[must_use]
    pub fn pinned(&self) -> usize {
        self.inside.lock().expect("the pool").pinned
    }

    /// Notes that a block was read off a file.
    ///
    /// Called by whatever did the reading rather than by the pool, which never
    /// reads anything itself: it is asked for a block and told when one had to
    /// be fetched.
    pub fn note_read(&self) {
        self.inside.lock().expect("the pool").faults.reads += 1;
    }

    /// Lets go of everything, keeping the room for what comes next.
    pub fn forget(&self) {
        let mut inside = self.inside.lock().expect("the pool");
        while let Some((_, gone)) = inside.kept.pop_lru() {
            recycle(&mut inside.scrap, gone);
        }
        inside.kept.clear();
        inside.bytes = 0;
    }

    /// A block of arcs to fill, emptied and with whatever room it had kept.
    #[must_use]
    pub fn take_arcs(&self) -> HeldArcs {
        let mut inside = self.inside.lock().expect("the pool");
        inside.scrap.arcs.pop().unwrap_or_default()
    }

    /// A table to fill, on the same terms.
    #[must_use]
    pub fn take_table(&self) -> HeldTable {
        let mut inside = self.inside.lock().expect("the pool");
        inside.scrap.tables.pop().unwrap_or_default()
    }

    /// A run of bytes at least this long, for reading a block off the file.
    ///
    /// It comes back the length asked for and holding nothing worth reading.
    #[must_use]
    pub fn take_bytes(&self, want: usize) -> Vec<u8> {
        let mut held = {
            let mut inside = self.inside.lock().expect("the pool");
            inside.scrap.bytes.pop().unwrap_or_default()
        };
        held.clear();
        held.resize(want, 0);
        held
    }

    /// Hands a run of bytes back, where there is room to keep it.
    pub fn give_bytes(&self, mut held: Vec<u8>) {
        let mut inside = self.inside.lock().expect("the pool");
        if inside.scrap.bytes.len() < SPARE {
            held.clear();
            inside.scrap.bytes.push(held);
        }
    }
}

/// Empties what is being let go of into the free list, where there is room.
///
/// Only what nobody else is holding: a caller may still have the thing that
/// was evicted, and the room is not free until it is done with it.
fn recycle(scrap: &mut Scrap, gone: Held) {
    match gone {
        Held::Arcs(arcs) if scrap.arcs.len() < SPARE => {
            if let Some(mut held) = Arc::into_inner(arcs) {
                held.empty();
                scrap.arcs.push(held);
            }
        }
        Held::Table(table) if scrap.tables.len() < SPARE => {
            if let Some(mut held) = Arc::into_inner(table) {
                held.empty();
                scrap.tables.push(held);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(nodes: usize) -> Held {
        Held::Way(Arc::new(vec![0 as NodeID; nodes]))
    }

    #[test]
    fn what_is_put_in_comes_back_out() {
        let pool = Pool::of(1 << 20);
        pool.put(Key::Way(1, 2, 0), way(8));
        match pool.get(&Key::Way(1, 2, 0)) {
            Some(Held::Way(held)) => assert_eq!(held.len(), 8),
            other => panic!("a way went in and {other:?} came out"),
        }
        assert!(pool.get(&Key::Way(9, 9, 0)).is_none());
        let faults = pool.faults();
        assert_eq!((faults.hits, faults.misses), (1, 1));
    }

    /// Two kinds may hold the same numbers, and must not be each other.
    #[test]
    fn a_name_of_one_kind_is_not_a_name_of_another() {
        let pool = Pool::of(1 << 20);
        pool.put(
            Key::Table(1, 2),
            Held::Table(Arc::new(HeldTable::default())),
        );
        assert!(pool.get(&Key::Block(1, 2)).is_none());
        assert!(pool.get(&Key::Arcs(1)).is_none());
        assert!(pool.get(&Key::Table(1, 2)).is_some());
    }

    /// The one that matters: the budget is over everything at once, so a run
    /// of one kind pushes out another kind rather than each keeping its own.
    #[test]
    fn one_budget_is_kept_across_every_kind_at_once() {
        let pool = Pool::of(16 * 1024);
        pool.put(Key::Arcs(0), Held::Arcs(Arc::new(HeldArcs::default())));
        for at in 0..64 {
            pool.put(Key::Way(at, at, 0), way(256));
        }
        let faults = pool.faults();
        assert!(faults.held <= 16 * 1024, "held {} bytes", faults.held);
        assert!(faults.evicted > 0, "nothing was let go of");
        assert!(
            pool.get(&Key::Arcs(0)).is_none(),
            "the ways did not push the arcs out"
        );
    }

    /// The one this is for: a run of reads asks the allocator for the room
    /// once and then keeps handing the same buffers round.
    #[test]
    fn the_room_a_read_wants_comes_back_from_the_one_before_it() {
        let pool = Pool::of(4 * 1024);

        // a run of blocks, each pushing the last out
        let mut seen = Vec::new();
        for at in 0..32 {
            let mut held = pool.take_arcs();
            held.pretend(64);
            seen.push(held.bytes());
            pool.put(Key::Arcs(at), Held::Arcs(Arc::new(held)));
        }
        assert!(pool.faults().evicted > 0, "nothing was let go of");

        // and the room comes back rather than being asked for again
        let recycled = pool.take_arcs();
        assert!(recycled.spare() > 0, "a buffer came back with no room kept");
        assert_eq!(recycled.edges(), 0, "and it came back holding something");
    }

    #[test]
    fn a_run_of_bytes_comes_back_the_length_asked_for_and_empty() {
        let pool = Pool::of(1 << 20);
        let mut first = pool.take_bytes(100);
        assert_eq!(first.len(), 100);
        first[0] = 7;
        pool.give_bytes(first);

        let second = pool.take_bytes(40);
        assert_eq!(second.len(), 40, "a different length than was asked for");
        assert!(second.iter().all(|&byte| byte == 0), "it held something");
    }

    #[test]
    fn something_larger_than_the_whole_budget_is_not_kept() {
        let pool = Pool::of(1024);
        pool.put(Key::Way(0, 0, 0), way(4096));
        assert!(pool.get(&Key::Way(0, 0, 0)).is_none());
        assert_eq!(pool.faults().held, 0, "the pool kept what it cannot hold");
    }

    #[test]
    fn forgetting_leaves_nothing_and_costs_nothing() {
        let pool = Pool::of(1 << 20);
        for at in 0..32 {
            pool.put(Key::Way(at, at, 0), way(16));
        }
        assert!(pool.faults().held > 0);
        pool.forget();
        assert_eq!(pool.faults().held, 0);
        assert!(pool.get(&Key::Way(0, 0, 0)).is_none());
    }

    /// The one this is for: what is pinned stays however much else goes
    /// through the pool.
    #[test]
    fn what_is_pinned_is_not_let_go_of() {
        let pool = Pool::of(16 * 1024);
        pool.pin(Key::Way(1, 1, 0), way(64));
        let stuck = pool.pinned();
        assert!(stuck > 0, "nothing was pinned");

        // enough of everything else to have emptied the list many times over
        for at in 0..256 {
            pool.put(Key::Way(at + 2, at + 2, 0), way(128));
        }
        assert!(pool.faults().evicted > 0, "nothing was let go of");
        assert!(
            pool.get(&Key::Way(1, 1, 0)).is_some(),
            "what was pinned went anyway"
        );
        assert_eq!(pool.pinned(), stuck, "what is pinned changed size");
        assert!(
            pool.faults().held <= 16 * 1024,
            "held {} bytes",
            pool.faults().held
        );
    }

    /// And it is the budget's: a pool whose pins fill it caches nothing.
    #[test]
    fn what_is_pinned_is_counted_against_the_budget() {
        let pool = Pool::of(4 * 1024);
        for at in 0..64 {
            pool.pin(Key::Way(at, at, 0), way(64));
        }
        assert!(pool.pinned() > 0);
        pool.put(Key::Arcs(0), way(64));
        assert!(
            pool.get(&Key::Arcs(0)).is_none(),
            "a pool with no room left kept something anyway"
        );
    }
}
