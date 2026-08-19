use std::process::Command;

fn main() {
    // note: add error checking yourself.
    let output = Command::new("git")
        .args(["describe", "--dirty"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    generate_vector_tile_bindings();
}

/// Generates the vector tile bindings that the tile server binary needs. The
/// .proto is compiled by protox rather than by protoc, so building this crate
/// does not depend on a protobuf compiler being installed.
#[cfg(feature = "tile_server")]
fn generate_vector_tile_bindings() {
    const PROTO: &str = "src/protos/vector_tile.proto";
    println!("cargo:rerun-if-changed={PROTO}");

    let file_descriptors =
        protox::compile([PROTO], ["src/protos"]).expect("vector tile proto does not compile");
    prost_build::Config::new()
        .compile_fds(file_descriptors)
        .expect("vector tile bindings cannot be generated");
}

/// Without the tile server there is nothing to generate, which keeps the codec
/// crates out of the build of anyone who only uses the library.
#[cfg(not(feature = "tile_server"))]
fn generate_vector_tile_bindings() {}
