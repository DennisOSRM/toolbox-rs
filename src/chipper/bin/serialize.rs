use bincode::serialize_into;
use std::{
    fs::File,
    io::{BufWriter, Write},
};
use toolbox_rs::{max_flow::ResidualCapacity, partition::PartitionID};

pub fn geometry_list(
    file_path: &str,
    edges: Vec<toolbox_rs::edge::InputEdge<ResidualCapacity>>,
    assignment: bitvec::prelude::BitVec,
    renumbering_table: Vec<usize>,
    coordinates: Vec<toolbox_rs::geometry::primitives::FPCoordinate>,
) {
    let mut file = File::create(file_path).expect("output file cannot be opened");
    file.write_all("latitude, longitude\n".as_bytes())
        .expect("error writing file");
    // fetch the cut and output its geometry
    for edge in &edges {
        if assignment[renumbering_table[edge.source]] != assignment[renumbering_table[edge.target]]
        {
            file.write_all(
                (coordinates[edge.source].lat as f64 / 1000000.)
                    .to_string()
                    .as_bytes(),
            )
            .expect("error writing file");
            file.write_all(b", ").expect("error writing file");
            file.write_all(
                (coordinates[edge.source].lon as f64 / 1000000.)
                    .to_string()
                    .as_bytes(),
            )
            .expect("error writing file");
            file.write_all(b"\n").expect("error writing file");

            file.write_all(
                (coordinates[edge.target].lat as f64 / 1000000.)
                    .to_string()
                    .as_bytes(),
            )
            .expect("error writing file");
            file.write_all(b", ").expect("error writing file");
            file.write_all(
                (coordinates[edge.target].lon as f64 / 1000000.)
                    .to_string()
                    .as_bytes(),
            )
            .expect("error writing file");
            file.write_all(b"\n").expect("error writing file");
        }
    }
    file.flush().expect("error writing file");
}

pub fn assignment_csv(
    filename: &str,
    partition_ids: &[PartitionID],
    coordinates: &[toolbox_rs::geometry::primitives::FPCoordinate],
) {
    let mut file = File::create(filename).expect("output file cannot be opened");
    file.write_all("partition_id, latitude, longitude\n".as_bytes())
        .expect("error writing file");
    for i in 0..partition_ids.len() {
        file.write_all(partition_ids[i].to_string().as_bytes())
            .expect("error writing file");
        file.write_all(b", ").expect("error writing file");

        file.write_all(
            (coordinates[i].lat as f64 / 1000000.)
                .to_string()
                .as_bytes(),
        )
        .expect("error writing file");
        file.write_all(b", ").expect("error writing file");
        file.write_all(
            (coordinates[i].lon as f64 / 1000000.)
                .to_string()
                .as_bytes(),
        )
        .expect("error writing file");
        file.write_all(b"\n").expect("error writing file");
    }
}

pub fn binary_partition_file(partition_file: &str, partition_ids: Vec<PartitionID>) {
    let mut f = BufWriter::new(File::create(partition_file).unwrap());
    serialize_into(&mut f, &partition_ids).unwrap();
}
