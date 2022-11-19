use crate::partition::PartitionID;

/// Note: LevelDirectory is a poor naming choice.
/// This struct encapsulates the logic to decide whether
///  - two nodes are within the same cell at a given level
///  - TBD.
pub struct LevelDirectory<'a> {
    partition_ids: &'a [PartitionID],
    levels: &'a [u32],
}

impl<'a> LevelDirectory<'a> {
    pub fn new(partition_ids: &'a [PartitionID], levels: &'a [u32]) -> Self {
        Self {
            partition_ids,
            levels,
        }
    }
    pub fn crosses_at_level(&self, u: usize, v: usize, level: u32) -> bool {
        let u_id = self.partition_ids[u];
        let v_id = self.partition_ids[v];
        let u_id = u_id.parent_at_level(level);
        let v_id = v_id.parent_at_level(level);
        u_id != v_id
    }

    // return a slice of all the levels where two nodes are in different cells
    pub fn get_crossing_levels(&self, u: usize, v: usize) -> &[u32] {
        let mut i = 0;
        for level in self.levels {
            if self.crosses_at_level(u, v, *level) {
                i += 1;
            } else {
                break;
            }
        }
        &self.levels[..i]
    }
}

// TODO: Add tests
// let id = PartitionID::new(0xffff_ffff);
// for l in &levels {
//     // TODO: remove
//     println!("[{l:#02}] {:#032b}", id.parent_at_level(*l));
// }
