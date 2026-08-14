//! A doubly linked list whose nodes live in one vector and name each other by
//! index. It carries the three operations an LRU cache is built on:
//!
//! 1) [`LinkedList::push_front`]: insert an element at the front of the list
//! 2) [`LinkedList::pop_back`]: remove the back element if there is one, and
//! 3) [`LinkedList::move_to_front`]: move an element that is already in the
//!    list to the front
//!
//! all of them in constant time, and each of them handing out or taking a
//! [`ListCursor`] that names a node for as long as it is in the list.
//!
//! # Why the nodes are indexed rather than pointed at
//!
//! Boxing each node and joining them with raw pointers is the usual way to
//! write this, and it is how this module started out. It costs `unsafe` on
//! every link, and it costs `Send`: a `NonNull` is not `Send`, so neither the
//! list nor anything built on it could be handed to another thread or shared
//! behind a lock. That ruled out the one use this list has, as the tile server
//! shares a single [`crate::lru::LRU`] of drawn tiles between its workers.
//!
//! Holding the nodes in a vector and naming them by position gives up nothing
//! a cache asks of a list, and leaves the module in safe code that a server can
//! share across threads. A popped node leaves its slot behind for the next push
//! to take, so a list held at a capacity settles at that many slots and stops
//! allocating.
//!
//! # Cursors outliving their node
//!
//! A cursor stays valid while its node is in the list. Popping the node ends
//! that, and what a cursor does afterwards depends on what became of its slot.
//! While the slot is still free, the cursor names nothing: [`LinkedList::get`]
//! reads it as `None` and [`LinkedList::move_to_front`] leaves the list alone.
//! Once a later push has taken the slot over, the cursor names that push's
//! element, which is a wrong answer rather than the undefined behaviour the
//! pointers used to give. Telling those two apart would cost a count per slot,
//! and a caller that hands out cursors is expected to drop them along with
//! their nodes anyway, which is what [`crate::lru::LRU`] does when it evicts.

use std::fmt::{self, Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Names a node of a [`LinkedList`] for as long as that node is in the list.
///
/// It is only a position in the list's vector. The element type it carries is
/// what keeps a cursor of one list from being handed to a list of another kind,
/// and costs nothing at run time.
pub struct ListCursor<T> {
    index: usize,
    /// `fn() -> T` rather than `T`, so that a cursor is `Send`, `Sync` and
    /// `Copy` whatever the element type is: it holds no element, it names one.
    _ghost: PhantomData<fn() -> T>,
}

impl<T> ListCursor<T> {
    const fn new(index: usize) -> Self {
        Self {
            index,
            _ghost: PhantomData,
        }
    }
}

// The derives would all demand `T: Trait`, which a cursor has no need of: it
// holds an index, not an element.
impl<T> Clone for ListCursor<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ListCursor<T> {}

impl<T> Debug for ListCursor<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ListCursor").field(&self.index).finish()
    }
}

impl<T> PartialEq for ListCursor<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for ListCursor<T> {}

impl<T> Hash for ListCursor<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

/// A slot of the list's vector: an element and the two nodes it sits between.
///
/// The links run by index, and the element is taken out of the slot when the
/// node is popped, which is what marks the slot as free.
pub struct Node<T> {
    /// the node one step towards the front, i.e. the more recently used one
    newer: Option<usize>,
    /// the node one step towards the back, i.e. the one used longer ago
    older: Option<usize>,
    /// the element, or `None` while the slot waits to be used again
    elem: Option<T>,
}

/// A doubly linked list over a vector of nodes. See the [module
/// documentation](self) for what it is for.
///
/// # Examples
///
/// ```
/// use toolbox_rs::linked_list::LinkedList;
///
/// let mut list = LinkedList::new();
/// list.push_front(1);
/// list.push_front(2);
/// list.push_front(3);
///
/// // the first element is the one that has waited longest
/// assert_eq!(list.pop_back(), Some(1));
///
/// // unless it is asked for again before the next pop
/// let mut list = LinkedList::new();
/// let first = list.push_front(1);
/// list.push_front(2);
/// list.move_to_front(&first);
/// assert_eq!(list.pop_back(), Some(2));
/// ```
pub struct LinkedList<T> {
    nodes: Vec<Node<T>>,
    front: Option<usize>,
    back: Option<usize>,
    /// the slots that pops have left behind, newest first
    free: Vec<usize>,
    len: usize,
}

impl<T> LinkedList<T> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            front: None,
            back: None,
            free: Vec::new(),
            len: 0,
        }
    }

    /// A list that can hold `capacity` nodes before it has to grow.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Vec::with_capacity(capacity),
            front: None,
            back: None,
            free: Vec::new(),
            len: 0,
        }
    }

    /// Takes a slot for a new node, reusing one that a pop left behind before
    /// growing the vector.
    fn claim_slot(&mut self, node: Node<T>) -> usize {
        if let Some(index) = self.free.pop() {
            debug_assert!(self.nodes[index].elem.is_none());
            self.nodes[index] = node;
            return index;
        }
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Inserts an element at the front of the list and hands back the cursor
    /// that names it.
    pub fn push_front(&mut self, elem: T) -> ListCursor<T> {
        let index = self.claim_slot(Node {
            newer: None,
            older: self.front,
            elem: Some(elem),
        });

        match self.front {
            Some(old_front) => self.nodes[old_front].newer = Some(index),
            // an empty list gains a back as well as a front
            None => self.back = Some(index),
        }
        self.front = Some(index);
        self.len += 1;

        ListCursor::new(index)
    }

    /// Moves the node a cursor names to the front of the list.
    ///
    /// Does nothing when the node is already at the front, and nothing when
    /// the cursor no longer names a node of this list, i.e. when its node has
    /// been popped without the slot being taken over since.
    pub fn move_to_front(&mut self, cursor: &ListCursor<T>) {
        let index = cursor.index;
        // A cursor whose node has been popped names a free slot, and one from
        // a list that has been cleared may name no slot at all. Moving either
        // of them would splice a free slot into the list, leaving it in the
        // list and on the free list at once for the next push to hand out from
        // under it. The list is checked rather than asserted over, as the
        // caller holding the stale cursor is the one who cannot tell.
        let occupied = self
            .nodes
            .get(index)
            .is_some_and(|node| node.elem.is_some());
        if !occupied || self.front == Some(index) {
            return;
        }

        // close the gap the node leaves behind
        let newer = self.nodes[index].newer;
        let older = self.nodes[index].older;
        if let Some(newer) = newer {
            self.nodes[newer].older = older;
        }
        if let Some(older) = older {
            self.nodes[older].newer = newer;
        }
        if self.back == Some(index) {
            // the node was the one that had waited longest, so whatever sat in
            // front of it has now
            debug_assert!(older.is_none());
            self.back = newer;
        }

        // and put it in front of what used to be the front. The list is not
        // empty and the node is not the front, so there is one.
        let front = self.front.expect("a list that is not empty has a front");
        self.nodes[index].newer = None;
        self.nodes[index].older = Some(front);
        self.nodes[front].newer = Some(index);
        self.front = Some(index);
    }

    /// Removes the element at the back of the list, i.e. the one that has gone
    /// longest without being asked for.
    pub fn pop_back(&mut self) -> Option<T> {
        let index = self.back?;
        let elem = self.nodes[index]
            .elem
            .take()
            .expect("a node in the list holds an element");

        self.back = self.nodes[index].newer;
        match self.back {
            Some(new_back) => self.nodes[new_back].older = None,
            // the list has run empty, so it has no front either
            None => self.front = None,
        }

        self.nodes[index].newer = None;
        self.nodes[index].older = None;
        self.free.push(index);
        self.len -= 1;

        Some(elem)
    }

    /// The element a cursor names, or `None` once its node has been popped.
    ///
    /// A slot that a later push has taken over holds that push's element, which
    /// is why a cursor is only worth keeping while its node is in the list.
    pub fn get(&self, cursor: &ListCursor<T>) -> Option<&T> {
        self.nodes.get(cursor.index)?.elem.as_ref()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Empties the list, keeping the room it has already taken so that filling
    /// it again does not allocate.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.front = None;
        self.back = None;
        self.len = 0;
    }

    /// The element at the front, i.e. the one most recently pushed or moved
    /// there.
    ///
    /// # Panics
    ///
    /// Panics if the list is empty.
    pub fn get_front(&self) -> &T {
        let front = self.front.expect("an empty list has no front");
        self.nodes[front]
            .elem
            .as_ref()
            .expect("a node in the list holds an element")
    }

    /// # Panics
    ///
    /// Panics if the list is empty.
    pub fn get_front_mut(&mut self) -> &mut T {
        let front = self.front.expect("an empty list has no front");
        self.nodes[front]
            .elem
            .as_mut()
            .expect("a node in the list holds an element")
    }
}

impl<T> Default for LinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use super::{LinkedList, ListCursor};

    /// Reads the list from the back, which is the order the pops come in.
    fn drained<T>(list: &mut LinkedList<T>) -> Vec<T> {
        let mut result = Vec::new();
        while let Some(element) = list.pop_back() {
            result.push(element);
        }
        result
    }

    #[test]
    fn default_init_cursor_noop() {
        let mut list = LinkedList::default();

        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
        let cursor = list.push_front(10);
        assert_eq!(list.len(), 1);
        assert_eq!(list.pop_back(), Some(10));
        list.move_to_front(&cursor); // no-op since list is empty
    }

    #[test]
    fn test_basic_front() {
        let mut list = LinkedList::new();

        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);

        list.push_front(10);
        assert_eq!(list.len(), 1);
        assert_eq!(list.pop_back(), Some(10));
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);

        list.push_front(10);
        assert_eq!(list.len(), 1);
        list.push_front(20);
        assert_eq!(list.len(), 2);
        list.push_front(30);
        assert_eq!(list.len(), 3);
        assert_eq!(list.pop_back(), Some(10));
        assert_eq!(list.len(), 2);
        list.push_front(40);
        assert_eq!(list.len(), 3);
        assert_eq!(list.pop_back(), Some(20));
        assert_eq!(list.len(), 2);
        assert_eq!(list.pop_back(), Some(30));
        assert_eq!(list.len(), 1);
        assert_eq!(list.pop_back(), Some(40));
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
        assert_eq!(list.pop_back(), None);
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn basic_move_to_front() {
        let mut list = LinkedList::new();

        assert_eq!(list.len(), 0);
        let first_inserted = list.push_front(1);
        list.move_to_front(&first_inserted);

        list.push_front(5);
        list.push_front(4);
        list.push_front(3);
        list.push_front(2);

        list.move_to_front(&first_inserted);

        list.push_front(0);

        assert_eq!(drained(&mut list), vec![5, 4, 3, 2, 1, 0]);
    }

    #[test]
    fn push_sort_move() {
        // test idea:
        // - nodes handles are stored in an array
        // - sort array by element
        // - run move_to_front on all elements in order of sorted array
        // - output should be sorted
        let mut list = LinkedList::new();
        let mut handles = Vec::new();
        assert_eq!(list.len(), 0);
        handles.push(list.push_front(1));
        handles.push(list.push_front(5));
        handles.push(list.push_front(2));
        handles.push(list.push_front(4));
        handles.push(list.push_front(3));

        handles.sort_by_key(|handle| *list.get(handle).expect("handle went stale"));

        handles.iter().for_each(|handle| {
            list.move_to_front(handle);
        });

        assert_eq!(drained(&mut list), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    #[should_panic]
    fn test_get_front_mut_empty() {
        let mut list: LinkedList<i32> = LinkedList::new();
        let _result = list.get_front_mut(); // Should panic on empty list
    }

    #[test]
    fn test_get_front_mut() {
        let mut list = LinkedList::new();

        // Test with one element
        list.push_front(10);
        {
            let front = list.get_front_mut();
            *front = 20;
        }
        assert_eq!(list.get_front(), &20);

        // Test with multiple elements
        list.push_front(30);
        list.push_front(40);
        {
            let front = list.get_front_mut();
            *front = 50;
        }
        assert_eq!(list.get_front(), &50);

        // Verify other elements are unchanged
        assert_eq!(list.pop_back(), Some(20));
    }

    #[test]
    fn moving_the_back_to_the_front_leaves_the_back_behind_it() {
        let mut list = LinkedList::new();
        let oldest = list.push_front(1);
        list.push_front(2);
        list.push_front(3);

        list.move_to_front(&oldest);

        // 2 has become the one that has waited longest
        assert_eq!(list.get_front(), &1);
        assert_eq!(drained(&mut list), vec![2, 3, 1]);
    }

    #[test]
    fn moving_the_front_to_the_front_changes_nothing() {
        let mut list = LinkedList::new();
        list.push_front(1);
        let front = list.push_front(2);

        list.move_to_front(&front);

        assert_eq!(list.len(), 2);
        assert_eq!(drained(&mut list), vec![1, 2]);
    }

    #[test]
    fn moving_a_node_out_of_the_middle_joins_its_neighbours() {
        let mut list = LinkedList::new();
        list.push_front(1);
        let middle = list.push_front(2);
        list.push_front(3);

        list.move_to_front(&middle);

        assert_eq!(drained(&mut list), vec![1, 3, 2]);
    }

    #[test]
    fn a_popped_slot_is_used_again_rather_than_grown_into() {
        let mut list = LinkedList::new();
        list.push_front(1);
        list.push_front(2);
        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.pop_back(), Some(2));

        // both slots are free, so filling the list again reuses them
        list.push_front(3);
        list.push_front(4);
        assert_eq!(list.nodes.len(), 2);
        assert_eq!(drained(&mut list), vec![3, 4]);
    }

    /// A stale cursor must not splice its slot back into the list. The slot is
    /// on the free list, so a list that took it back would hand it to the next
    /// push while it was still linked, and the damage would only show up in a
    /// later pop.
    #[test]
    fn moving_a_node_that_was_popped_leaves_the_list_alone() {
        let mut list = LinkedList::new();
        let popped = list.push_front(1);
        list.push_front(2);
        list.push_front(3);
        assert_eq!(list.pop_back(), Some(1));

        list.move_to_front(&popped);

        assert_eq!(list.len(), 2);
        assert_eq!(list.get_front(), &3);
        assert_eq!(drained(&mut list), vec![2, 3]);
    }

    #[test]
    fn a_cursor_from_before_a_clear_leaves_the_list_alone() {
        let mut list = LinkedList::new();
        list.push_front(1);
        list.push_front(2);
        let far = list.push_front(3);

        // clear gives up the slots, so the cursor now names one that is not
        // there at all rather than one that is free
        list.clear();
        list.push_front(4);
        list.move_to_front(&far);

        assert_eq!(list.len(), 1);
        assert_eq!(list.get_front(), &4);
        assert_eq!(drained(&mut list), vec![4]);
    }

    #[test]
    fn a_cursor_whose_node_was_popped_holds_nothing() {
        let mut list = LinkedList::new();
        let cursor = list.push_front(1);
        assert_eq!(list.get(&cursor), Some(&1));

        assert_eq!(list.pop_back(), Some(1));
        assert_eq!(list.get(&cursor), None);
    }

    #[test]
    fn clear_empties_the_list() {
        let mut list = LinkedList::new();
        list.push_front(1);
        list.push_front(2);

        list.clear();

        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
        assert_eq!(list.pop_back(), None);
        // and it can be filled again afterwards
        list.push_front(3);
        assert_eq!(list.get_front(), &3);
    }

    #[test]
    fn a_list_of_elements_that_are_not_copy_is_still_a_list() {
        let mut list = LinkedList::new();
        list.push_front("first".to_owned());
        let second = list.push_front("second".to_owned());

        // a cursor is Copy even when the element it names is not
        let also_second: ListCursor<String> = second;
        list.move_to_front(&also_second);

        assert_eq!(list.get_front(), "second");
        assert_eq!(drained(&mut list), vec!["first", "second"]);
    }

    /// The list is only worth having if it can be handed to another thread,
    /// which is what the whole module is indexed rather than pointed at for.
    #[test]
    fn a_list_and_its_cursors_can_be_sent_between_threads() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<LinkedList<usize>>();
        assert_sync::<LinkedList<usize>>();
        assert_send::<ListCursor<usize>>();
        assert_sync::<ListCursor<usize>>();

        let mut list = LinkedList::new();
        list.push_front(1);
        let moved = std::thread::spawn(move || {
            let mut list = list;
            list.push_front(2);
            drained(&mut list)
        })
        .join()
        .expect("the thread holding the list panicked");
        assert_eq!(moved, vec![1, 2]);
    }
}
