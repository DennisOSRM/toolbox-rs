mod command_line;
use std::{fs::File, io::BufWriter};

use bincode::encode_into_std_write;
use env_logger::Env;
use log::info;

use crate::command_line::{Arguments, InputFormat};
use toolbox_rs::{ddsg, dimacs, edge::InputEdge, metis};

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#"     ___     _       _                    "#);
    println!(r#"    | _ \   | |     (_)     ___      _ _  "#);
    println!(r#"    |  _/   | |     | |    / -_)    | '_| "#);
    println!(r#"   _|_|_   _|_|_   _|_|_   \___|   _|_|_  "#);
    println!(r#" _| """ |_|"""""|_|"""""|_|"""""|_|"""""| "#);
    println!(r#" "`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-' "#);
    println!("build: {}", env!("GIT_HASH"));

    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let edges: Vec<InputEdge<usize>> = match args.input_format {
        InputFormat::Ddsg => ddsg::read_graph(&args.graph, ddsg::WeightType::Original),
        InputFormat::Dimacs => dimacs::read_graph(&args.graph, dimacs::WeightType::Original),
        InputFormat::Metis => metis::read_graph(&args.graph, metis::WeightType::Original),
    };

    let coordinates = match args.input_format {
        InputFormat::Ddsg => ddsg::read_coordinates(&args.coordinates),
        InputFormat::Dimacs => dimacs::read_coordinates(&args.coordinates),
        InputFormat::Metis => metis::read_coordinates(&args.coordinates),
    };

    let config = bincode::config::standard();

    info!("writing edges into intermediate format");
    let mut f = BufWriter::new(File::create(args.graph + ".toolbox").unwrap());
    encode_into_std_write(&edges, &mut f, config).unwrap();

    info!("writing coordinates into intermediate format");
    let mut f = BufWriter::new(File::create(args.coordinates + ".toolbox").unwrap());
    encode_into_std_write(&coordinates, &mut f, config).unwrap();

    info!("done.");
}
