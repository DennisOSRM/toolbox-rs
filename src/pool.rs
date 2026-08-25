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
}

/// What one thing in the pool is.
#[derive(Clone, Debug)]
pub enum Held {
    Arcs(Arc<HeldArcs>),
    Table(Arc<HeldTable>),
    Block(Arc<CellBlock>),
    Way(Arc<Vec<NodeID>>),
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
            Self::Block(block) => block.framing_bytes(),
            Self::Way(way) => way.capacity() * size_of::<NodeID>(),
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
    /// what the pool is holding, in bytes
    pub held: usize,
    /// the most it has ever held
    pub highest: usize,
}

struct Inside {
    kept: LRU<Key, Held>,
    bytes: usize,
    faults: Faults,
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
                bytes: 0,
                faults: Faults::default(),
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
            held: inside.bytes,
            ..inside.faults
        }
    }

    /// Whatever is under this name, where anything is.
    #[must_use]
    pub fn get(&self, key: &Key) -> Option<Held> {
        let mut inside = self.inside.lock().expect("the pool");
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
        while inside.bytes + cost > self.budget
            && let Some((_, gone)) = inside.kept.pop_lru()
        {
            inside.bytes -= gone.bytes();
            inside.faults.evicted += 1;
        }
        inside.kept.push(&key, held);
        inside.bytes += cost;
        inside.faults.highest = inside.faults.highest.max(inside.bytes);
    }

    /// Lets go of everything.
    pub fn forget(&self) {
        let mut inside = self.inside.lock().expect("the pool");
        inside.kept.clear();
        inside.bytes = 0;
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
}
