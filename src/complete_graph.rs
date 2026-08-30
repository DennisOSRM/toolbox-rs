use std::fmt;
use std::ops::{Index, IndexMut};

/// A complete graph implementation where distances between nodes are stored in a
/// matrix including diagonal elements with zero distances.
///
/// This structure efficiently stores pairwise distances between all nodes in a
/// complete graph.
pub struct CompleteGraph<T> {
    /// Number of nodes in the graph
    num_nodes: usize,
    /// Vector to store the distances between nodes
    /// Includes diagonal elements (i,i) which are set to the zero value of T
    distances: Vec<T>,
}

impl<T: Clone + Default + PartialEq + fmt::Debug> CompleteGraph<T> {
    /// Creates a new complete graph with the specified number of nodes.
    ///
    /// # Arguments
    ///
    /// * `num_nodes` - The number of nodes in the complete graph
    ///
    /// # Returns
    ///
    /// A new CompleteGraph with default values for all distances and zero for diagonal
    pub fn new(num_nodes: usize) -> Self {
        // For a complete graph with n nodes, we need n*n entries to store all distances
        // This includes the diagonal elements (i,i)
        let size = num_nodes * num_nodes;
        Self {
            num_nodes,
            distances: vec![T::default(); size],
        }
    }

    /// Creates a new complete graph from a vector of distances.
    ///
    /// # Arguments
    ///
    /// * `num_nodes` - The number of nodes in the complete graph
    /// * `distances` - A vector containing all pairwise distances in triangular format
    /// * `zero` - The value to use for diagonal elements
    ///
    /// # Returns
    ///
    /// A new CompleteGraph with distances from the provided vector
    ///
    /// # Panics
    ///
    /// Panics if the vector size doesn't match the expected size for num_nodes
    pub fn from_vec(num_nodes: usize, distances: Vec<T>) -> Self {
        let expected_size = num_nodes * num_nodes;
        assert_eq!(
            distances.len(),
            expected_size,
            "Vector length {} doesn't match expected size {} for {} nodes. For a complete graph with n nodes including diagonal elements, we need n*n distances.",
            distances.len(),
            expected_size,
            num_nodes
        );

        Self {
            num_nodes,
            distances,
        }
    }

    /// Returns the number of nodes in the graph.
    pub fn num_nodes(&self) -> usize {
        self.num_nodes
    }

    /// Converts a pair of node indices to a flat array index.
    ///
    /// # Arguments
    ///
    /// * `i` - First node index
    /// * `j` - Second node index
    ///
    /// # Returns
    ///
    /// The flat array index
    #[inline(always)]
    fn get_index(&self, i: usize, j: usize) -> usize {
        // Not a debug_assert. The row is folded into the column here, so a j
        // past the end lands on a valid element of another row rather than off
        // the end of the vector: for three nodes, (0, 4) is index 4, which is
        // (1, 1). Nothing below this catches that.
        assert!(
            i < self.num_nodes && j < self.num_nodes,
            "node indices ({i}, {j}) out of bounds for {} nodes",
            self.num_nodes
        );
        i * self.num_nodes + j
    }

    /// Gets a reference to the distance between two nodes.
    ///
    /// # Arguments
    ///
    /// * `i` - First node index
    /// * `j` - Second node index
    ///
    /// # Returns
    ///
    /// A reference to the distance between nodes i and j
    pub fn get(&self, i: usize, j: usize) -> &T {
        let idx = self.get_index(i, j);
        &self.distances[idx]
    }

    /// Gets a mutable reference to the distance between two nodes.
    ///
    /// # Arguments
    ///
    /// * `i` - First node index
    /// * `j` - Second node index
    ///
    /// # Returns
    ///
    /// A mutable reference to the distance between nodes i and j
    pub fn get_mut(&mut self, i: usize, j: usize) -> &mut T {
        let idx = self.get_index(i, j);
        &mut self.distances[idx]
    }
}

// Implement Index and IndexMut to allow graph[i, j] syntax
impl<T: Clone + Default + PartialEq + fmt::Debug> Index<(usize, usize)> for CompleteGraph<T> {
    type Output = T;

    fn index(&self, index: (usize, usize)) -> &Self::Output {
        self.get(index.0, index.1)
    }
}

impl<T: Clone + Default + PartialEq + fmt::Debug> IndexMut<(usize, usize)> for CompleteGraph<T> {
    fn index_mut(&mut self, index: (usize, usize)) -> &mut Self::Output {
        if index.0 == index.1 {
            panic!("Cannot modify diagonal");
        }
        // If the index is not diagonal, return a mutable reference
        self.get_mut(index.0, index.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_graph_creation() {
        let graph: CompleteGraph<f32> = CompleteGraph::new(5);
        assert_eq!(graph.num_nodes(), 5);
    }

    #[test]
    fn test_diagonal_elements() {
        let graph: CompleteGraph<i32> = CompleteGraph::new(4);

        // Check diagonal elements are zero
        assert_eq!(graph[(0, 0)], 0);
        assert_eq!(graph[(1, 1)], 0);
        assert_eq!(graph[(2, 2)], 0);
        assert_eq!(graph[(3, 3)], 0);
    }

    #[test]
    fn test_get_set_distance() {
        let mut graph: CompleteGraph<i32> = CompleteGraph::new(4);

        // Set some distances
        *graph.get_mut(0, 1) = 10;
        *graph.get_mut(1, 2) = 20;
        *graph.get_mut(2, 3) = 30;

        // Check distances
        assert_eq!(*graph.get(0, 1), 10);
        assert_eq!(*graph.get(1, 2), 20);
        assert_eq!(*graph.get(2, 3), 30);

        // Check diagonal elements
        assert_eq!(*graph.get(0, 0), 0);
        assert_eq!(*graph.get(1, 1), 0);
    }

    #[test]
    fn test_index_syntax() {
        let mut graph: CompleteGraph<i32> = CompleteGraph::new(4);

        // Set values using the index syntax
        graph[(0, 1)] = 10;
        graph[(1, 2)] = 20;

        // Check values using the index syntax
        assert_eq!(graph[(0, 1)], 10);
        assert_eq!(graph[(1, 2)], 20);
        assert_eq!(graph[(0, 0)], 0);
    }

    #[test]
    fn test_from_vec_constructor() {
        // For 4 nodes with diagonal, we need 10 distances:
        // (0,0), (0,1), (0,2), (0,3), (1,1), (1,2), (1,3), (2,2), (2,3), (3,3)
        let distances = &[0, 10, 20, 30, 0, 40, 50, 60, 0];
        let graph = CompleteGraph::from_vec(3, distances.to_vec());

        assert_eq!(graph.num_nodes(), 3);
        assert_eq!(graph[(0, 0)], 0);
        assert_eq!(graph[(0, 1)], 10);
        assert_eq!(graph[(0, 2)], 20);
        assert_eq!(graph[(1, 0)], 30);
        assert_eq!(graph[(1, 1)], 0);
        assert_eq!(graph[(1, 2)], 40);
        assert_eq!(graph[(2, 0)], 50);
        assert_eq!(graph[(2, 1)], 60);
        assert_eq!(graph[(2, 2)], 0);
    }

    #[test]
    #[should_panic(expected = "Cannot modify diagonal")]
    fn test_modify_diagonal() {
        let mut graph: CompleteGraph<i32> = CompleteGraph::new(4);
        graph[(0, 0)] = 5; // Should panic
    }

    #[test]
    #[should_panic(expected = "Vector length")]
    fn test_from_vec_wrong_size() {
        // For 4 nodes with diagonal, we need 10 distances, not 9
        let distances = vec![0, 10, 20, 30, 0, 40, 50, 0, 60];
        let _graph = CompleteGraph::<i32>::from_vec(4, distances);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_get_index_out_of_bounds() {
        let graph: CompleteGraph<i32> = CompleteGraph::new(3);

        // (3, 0) folds to index 9, which is off the end of the vector, so this
        // one panicked in release even before get_index asserted.
        let _ = graph[(3, 0)];
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_get_mut_index_out_of_bounds() {
        let mut graph: CompleteGraph<i32> = CompleteGraph::new(3);

        // (0, 4) folds to index 4, which is a valid element of the vector -- it
        // is (1, 1) -- so the vector access does not catch this and the assert
        // in get_index has to. In debug and in release alike.
        *graph.get_mut(0, 4) = 10;
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn test_get_column_out_of_bounds() {
        let graph: CompleteGraph<i32> = CompleteGraph::new(3);
        graph.get(0, 4);
    }
}
