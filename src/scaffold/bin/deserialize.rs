use std::{fs::File, io::BufReader};

use toolbox_rs::partition::PartitionID;

pub fn binary_partition_file(partition_file: &str) -> Vec<PartitionID> {
    let f = BufReader::new(File::open(partition_file).unwrap());
    let result = bincode::deserialize_from(f);
    result.unwrap()
}
