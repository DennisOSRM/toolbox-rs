use std::{cmp::PartialOrd, fmt::Debug};

#[derive(Debug)]
struct Node<T> {
    next: Option<Box<Node<T>>>,
    elem: T,
}

pub struct SingleLinkedList<T> {
    head: Option<Box<Node<T>>>,
}

impl<T: Copy + Debug + PartialOrd> SingleLinkedList<T> {
    pub fn new() -> Self {
        Self { head: None }
    }

    pub fn push_front(&mut self, elem: T) {
        let new_node = Box::new(Node {
            next: self.head.take(),
            elem,
        });
        self.head = Some(new_node);
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.head.take().map(|node| {
            self.head = node.next;
            node.elem
        })
    }

    pub fn peek_front(&self) -> Option<&T> {
        self.head.as_ref().map(|node| &node.elem)
    }

    pub fn peek_front_mut(&mut self) -> Option<&mut T> {
        self.head.as_mut().map(|node| &mut node.elem)
    }

    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub fn is_sorted(&self) -> bool {
        let mut current = &self.head;
        while let Some(node) = current {
            if let Some(next_node) = &node.next {
                if node.elem > next_node.elem {
                    return false;
                }
            }
            current = &node.next;
        }
        true
    }

    // Insert an element in a descendengly sorted linked list
    pub fn insert_sorted(&mut self, elem: T) {
        let mut current = &mut self.head;
        while let Some(node) = current {
            let next_is_smaller = match &node.next {
                Some(next) => next.elem < elem,
                None => false,
            };

            if next_is_smaller {
                current = &mut node.next;
            } else {
                let new_node = Box::new(Node {
                    next: node.next.take(),
                    elem,
                });
                node.next = Some(new_node);
                return;
            }
        }
    }
}

impl<T: Copy + Debug + PartialOrd> Default for SingleLinkedList<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    #[test]
    fn creation_push_peek_pop() {
        let mut list = super::SingleLinkedList::new();
        assert!(list.is_empty());
        list.push_front(1);
        list.push_front(2);
        list.push_front(3);
        assert!(!list.is_empty());
        assert_eq!(list.peek_front(), Some(&3));
        assert_eq!(list.peek_front_mut(), Some(&mut 3));
        assert_eq!(list.pop_front(), Some(3));
        assert_eq!(list.pop_front(), Some(2));
        assert_eq!(list.pop_front(), Some(1));
        assert_eq!(list.pop_front(), None);
        assert!(list.is_empty());
    }

    #[test]
    fn find_not_less() {
        let mut list = super::SingleLinkedList::new();
        assert!(list.is_empty());
        assert!(list.is_sorted());
        list.push_front(8);
        list.push_front(5);
        list.push_front(1);
        assert!(list.is_sorted());

        list.insert_sorted(3);
        list.insert_sorted(2);
        assert!(list.is_sorted());
        list.insert_sorted(6);
        list.insert_sorted(4);
        list.insert_sorted(7);
        list.insert_sorted(9);
    }

    #[test]
    fn unsorted() {
        let mut list = super::SingleLinkedList::default();
        assert!(list.is_empty());
        assert!(list.is_sorted());
        list.push_front(5);
        list.push_front(8);
        list.push_front(1);
        assert!(!list.is_sorted());
    }
}
