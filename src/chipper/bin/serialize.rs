use bincode::serialize_into;
use log::info;
use std::{
    fs::File,
    io::{BufWriter, Write},
};
use toolbox_rs::{edge::TrivialEdge, geometry::primitives::FPCoordinate, partition::PartitionID};

use crate::command_line::Arguments;

pub fn cut_csv(
    file_path: &str,
    edges: &[TrivialEdge],
    partition_ids: &[PartitionID],
    coordinates: &[FPCoordinate],
) {
    let mut file = BufWriter::new(File::create(file_path).expect("output file cannot be opened"));
    file.write_all("latitude, longitude\n".as_bytes())
        .expect("error writing file");
    // fetch the cut and output its geometry
    for edge in edges {
        if partition_ids[edge.source] != partition_ids[edge.target] {
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

pub fn assignment_csv(filename: &str, partition_ids: &[PartitionID], coordinates: &[FPCoordinate]) {
    let mut file = BufWriter::new(File::create(filename).expect("output file cannot be opened"));
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

pub fn binary_partition_file(partition_file: &str, partition_ids: &[PartitionID]) {
    let mut f = BufWriter::new(File::create(partition_file).unwrap());
    serialize_into(&mut f, &partition_ids).unwrap();
}

pub fn write_results(
    args: &Arguments,
    partition_ids: &[PartitionID],
    coordinates: &[FPCoordinate],
    edges: &[TrivialEdge],
) {
    if !args.assignment_csv.is_empty() {
        info!("writing partition csv into: {}", args.assignment_csv);
        assignment_csv(&args.assignment_csv, partition_ids, coordinates);
    }
    if !args.cut_csv.is_empty() {
        info!("writing cut csv to {}", &args.cut_csv);
        cut_csv(&args.cut_csv, edges, partition_ids, coordinates);
    }
    if !args.partition_file.is_empty() {
        info!("writing partition ids to {}", &args.partition_file);
        binary_partition_file(&args.partition_file, partition_ids);
    }
}
