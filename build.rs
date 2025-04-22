use std::{env, process::Command};
fn main() {
    // note: add error checking yourself.
    let output = Command::new("git")
        .args(["describe", "--dirty"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    println!("cargo:rustc-env=GIT_HASH={git_hash}");

    prost_build::compile_protos(&["src/protos/vector_tile.proto"], &["src/protos"]).unwrap();
    println!("cargo:rerun-if-changed=src/protos/vector_tile.proto");
    println!("cargo:warning=OUT_DIR is: {}", env::var("OUT_DIR").unwrap());
}
