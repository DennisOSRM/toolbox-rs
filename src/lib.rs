pub mod binary_heap;
pub mod static_graph;

#[cfg(test)]
mod tests {
    use crate::{binary_heap::BinaryHeap, static_graph::{InputEdge, StaticGraph}};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn size_of_graph() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];
        let graph = Graph::new(edges);

        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());
    }

    #[test]
    fn static_graph_degree() {

    }

    #[test]
    fn cycle_check_no_cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];
        let graph = Graph::new(edges);
        assert_eq!(false, graph.cycle_check());
    }

    #[test]
    fn cycle_check_cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(2, 3, 3),
            InputEdge::new(3, 4, 1),
            InputEdge::new(4, 5, 6),
            InputEdge::new(5, 2, 2),
        ];
        let graph = Graph::new(edges);
        assert_eq!(true, graph.cycle_check());
    }

    #[test]
    fn binary_heap_empty() {
        type Heap = BinaryHeap<i32>;
        let heap = Heap::new();

        assert!(heap.is_empty());
    }

    #[test]
    fn binary_heap_insert_size() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();
        heap.insert(20);
        assert_eq!(20, *heap.min());
        assert!(!heap.is_empty());
    }

    #[test]
    fn binary_heap_sort() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let mut input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 5);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn binary_heap_sort_random() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let mut rng = StdRng::seed_from_u64(0xAAaaAAaa);

        let mut input = Vec::new();
        for _ in 0..1000 {
            let number = rng.gen();
            input.push(number);
            heap.insert(number);
        }
        assert!(!heap.is_empty());
        assert_eq!(1000, heap.len());
        assert_eq!(1000, input.len());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 1000);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn binary_heap_clear() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        heap.clear();
        assert_eq!(0, heap.len());
    }

    #[test]
    #[should_panic]
    fn binary_heap_empty_min_panic() {
        type Heap = BinaryHeap<i32>;
        let heap = Heap::new();
        heap.min();
    }
}
