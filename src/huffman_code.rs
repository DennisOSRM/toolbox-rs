use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt::Debug;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Clone)]
pub struct HuffmanNode<T> {
    character: Option<T>,
    frequency: i32,
    left: Option<HuffmanNodeRef<T>>,
    right: Option<HuffmanNodeRef<T>>,
}

impl<T> PartialEq for HuffmanNode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.frequency == other.frequency
    }
}

impl<T> Eq for HuffmanNode<T> {}

impl<T> PartialOrd for HuffmanNode<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.frequency.partial_cmp(&other.frequency)
    }
}

impl<T> Ord for HuffmanNode<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.frequency.cmp(&other.frequency)
    }
}

type HuffmanNodeRef<T> = Rc<RefCell<HuffmanNode<T>>>;

fn min_node<T>(
    q1: &mut VecDeque<HuffmanNode<T>>,
    q2: &mut VecDeque<HuffmanNode<T>>,
) -> HuffmanNode<T> {
    if q1.is_empty() {
        return q2.pop_front().unwrap();
    }

    if q2.is_empty() {
        return q1.pop_front().unwrap();
    }

    if q1.front().unwrap().frequency < q2.front().unwrap().frequency {
        return q1.pop_front().unwrap();
    }
    q2.pop_front().unwrap()
}

pub fn generate_huffman_code_from_sorted<T: Copy + Debug>(
    v: &[(T, i32)],
) -> Vec<(T, std::string::String)> {
    if v.is_empty() {
        return Vec::new();
    }

    let mut q1 = VecDeque::new();
    let mut q2 = VecDeque::new();

    for (t, f) in v {
        q1.push_back(HuffmanNode::<T> {
            character: Some(*t),
            frequency: *f,
            left: None,
            right: None,
        })
    }

    while !q1.is_empty() || q2.len() > 1 {
        let left = min_node(&mut q1, &mut q2);
        let right = min_node(&mut q1, &mut q2);
        let node = HuffmanNode::<T> {
            character: None, // None signifies that this is an interior node
            frequency: left.frequency + right.frequency,
            left: Some(Rc::new(RefCell::new(left))),
            right: Some(Rc::new(RefCell::new(right))),
        };
        q2.push_back(node);
    }

    let root = Rc::new(RefCell::new(q2.pop_front().unwrap()));

    retrieve_codebook(root)
}

fn retrieve_codebook<T: Copy + Debug>(root: Rc<RefCell<HuffmanNode<T>>>) -> Vec<(T, String)> {
    // generate code book
    let mut code_book = Vec::new();
    let mut stack = Vec::new();
    stack.push((root.clone(), String::new()));

    while let Some((current, prefix)) = stack.pop() {
        if let Some(character) = current.borrow().character {
            code_book.push((character, prefix));
        } else {
            if let Some(left) = current.borrow().left.clone() {
                stack.push((left, prefix.clone() + "0"));
            }
            if let Some(right) = current.borrow().right.clone() {
                stack.push((right, prefix.clone() + "1"));
            }
        };
    }
    code_book
}

pub fn generate_huffman_code_from_unsorted<T: Copy + Debug>(
    v: &[(T, i32)],
) -> Vec<(T, std::string::String)> {
    // TODO: add yet another implementation of using sort() + sorted construction. Include criterion numbers
    if v.is_empty() {
        return Vec::new();
    }

    let mut q1 = BinaryHeap::new();

    for (t, f) in v {
        q1.push(Reverse(Rc::new(RefCell::new(HuffmanNode::<T> {
            character: Some(*t),
            frequency: *f,
            left: None,
            right: None,
        }))));
    }

    while q1.len() > 1 {
        let Reverse(x) = q1.pop().unwrap();
        let Reverse(y) = q1.pop().unwrap();

        let f1 = x.borrow().frequency;
        let f2 = y.borrow().frequency;

        let node = Rc::new(RefCell::new(HuffmanNode::<T> {
            character: None,
            frequency: f1 + f2,
            left: Some(x),
            right: Some(y),
        }));
        q1.push(Reverse(node));
    }

    let Reverse(root) = q1.pop().unwrap();
    retrieve_codebook(root)
}

#[cfg(test)]
mod tests {
    use super::generate_huffman_code_from_sorted;
    use super::generate_huffman_code_from_unsorted;

    #[test]
    fn construction_unsorted() {
        let v = [
            ('a', 5),
            ('b', 9),
            ('c', 12),
            ('d', 13),
            ('e', 16),
            ('f', 45),
        ];
        let code_book = generate_huffman_code_from_unsorted(&v);
        let expected = [
            ('f', "0"),
            ('c', "100"),
            ('d', "101"),
            ('a', "1100"),
            ('b', "1101"),
            ('e', "111"),
        ];

        let matching = code_book
            .iter()
            .rev()
            .zip(&expected)
            .filter(|&(a, b)| a.0 == b.0 && a.1 == b.1)
            .count();

        assert!(matching == code_book.len());
        assert!(matching == expected.len());
    }

    #[test]
    fn construction_sorted() {
        let v = [
            ('a', 5),
            ('b', 9),
            ('c', 12),
            ('d', 13),
            ('e', 16),
            ('f', 45),
        ];
        let code_book = generate_huffman_code_from_sorted(&v);
        let expected = [
            ('f', "0"),
            ('c', "100"),
            ('d', "101"),
            ('a', "1100"),
            ('b', "1101"),
            ('e', "111"),
        ];

        let matching = code_book
            .iter()
            .rev()
            .zip(&expected)
            .filter(|&(a, b)| a.0 == b.0 && a.1 == b.1)
            .count();

        assert!(matching == code_book.len());
        assert!(matching == expected.len());
    }
}
