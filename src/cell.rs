use crate::graph::NodeID;

/// A Cell represents a partition of a graph with border nodes and their distance matrix.
///
/// The Cell struct maintains a collection of border nodes (nodes that connect this cell to other cells)
/// and a distance matrix that stores the shortest distances between these border nodes within the cell.
///
/// # Examples
///
/// ```
/// use toolbox_rs::cell::Cell;
///
/// let border_nodes = vec![0, 1, 2]; // Three border nodes
/// let distances = vec![
///     0, 5, 7,  // distances from node 0 to others
///     5, 0, 3,  // distances from node 1 to others
///     7, 3, 0   // distances from node 2 to others
/// ];
/// let cell = Cell::new(border_nodes, distances, 42);
///
/// assert_eq!(cell.get_distance(0, 1), 5); // Distance from node 0 to 1
/// assert_eq!(cell.id(), 42);
/// assert_eq!(cell.border_nodes(), &[0, 1, 2]);
/// ```
#[derive(Clone, Debug, Default)]
pub struct Cell {
    border_nodes: Vec<NodeID>,
    distance_matrix: Vec<usize>,
    id: usize,
}

impl Cell {
    /// Creates a new Cell with the specified border nodes, distance matrix, and ID.
    ///
    /// # Arguments
    ///
    /// * `border_nodes` - Vector of node IDs that represent the border nodes of this cell
    /// * `distance_matrix` - A flattened matrix of distances between border nodes
    /// * `id` - Unique identifier for this cell
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::cell::Cell;
    ///
    /// let cell = Cell::new(vec![0, 1], vec![0, 4, 4, 0], 1);
    /// assert_eq!(cell.get_distance(0, 1), 4);
    /// ```
    pub fn new(border_nodes: Vec<NodeID>, distance_matrix: Vec<usize>, id: usize) -> Self {
        Self {
            border_nodes,
            distance_matrix,
            id,
        }
    }

    /// Returns the distance between two border nodes within the cell.
    ///
    /// # Arguments
    ///
    /// * `source` - Index of the source node in the border_nodes list
    /// * `target` - Index of the target node in the border_nodes list
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::cell::Cell;
    ///
    /// let cell = Cell::new(
    ///     vec![10, 20, 30],           // border nodes
    ///     vec![0, 5, 8, 5, 0, 3, 8, 3, 0], // 3x3 distance matrix
    ///     1
    /// );
    /// assert_eq!(cell.get_distance(0, 1), 5); // Distance from first to second border node
    /// assert_eq!(cell.get_distance(1, 2), 3); // Distance from second to third border node
    /// ```
    pub fn get_distance(&self, source: usize, target: usize) -> usize {
        self.distance_matrix[source * self.border_nodes.len() + target]
    }

    /// Returns the unique identifier of this cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::cell::Cell;
    ///
    /// let cell = Cell::new(vec![0], vec![0], 42);
    /// assert_eq!(cell.id(), 42);
    /// ```
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns a slice containing all border nodes of this cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::cell::Cell;
    ///
    /// let cell = Cell::new(vec![1, 2, 3], vec![0, 1, 1, 1, 0, 1, 1, 1, 0], 1);
    /// assert_eq!(cell.border_nodes(), &[1, 2, 3]);
    /// ```
    pub fn border_nodes(&self) -> &[NodeID] {
        &self.border_nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_cell() {
        let border_nodes = vec![1, 2, 3];
        let distance_matrix = vec![0, 1, 2, 1, 0, 3, 2, 3, 0];
        let id = 1;
        let cell = Cell::new(border_nodes.clone(), distance_matrix.clone(), id);

        assert_eq!(cell.border_nodes, border_nodes);
        assert_eq!(cell.distance_matrix, distance_matrix);
        assert_eq!(cell.id, id);
    }

    #[test]
    fn test_get_distance() {
        let cell = Cell::new(vec![1, 2, 3], vec![0, 4, 7, 4, 0, 2, 7, 2, 0], 1);

        assert_eq!(cell.get_distance(0, 1), 4);
        assert_eq!(cell.get_distance(1, 2), 2);
        assert_eq!(cell.get_distance(0, 2), 7);
        assert_eq!(cell.get_distance(2, 0), 7);
    }

    #[test]
    fn test_border_nodes() {
        let nodes = vec![1, 2, 3, 4];
        let cell = Cell::new(
            nodes.clone(),
            vec![0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 1, 0],
            1,
        );

        assert_eq!(cell.border_nodes(), &nodes);
    }

    #[test]
    fn test_cell_id() {
        let cell = Cell::new(vec![1], vec![0], 42);
        assert_eq!(cell.id(), 42);
    }

    #[test]
    fn test_cell_clone() {
        let original = Cell::new(vec![1, 2], vec![0, 1, 1, 0], 1);
        let cloned = original.clone();

        assert_eq!(original.border_nodes(), cloned.border_nodes());
        assert_eq!(original.id(), cloned.id());
        assert_eq!(original.get_distance(0, 1), cloned.get_distance(0, 1));
    }
}
