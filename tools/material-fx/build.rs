//! Numbers the showcase's control operations for the tests that compile the client part's
//! effect table natively, as the client part's own build script does.

#[path = "../../examples/experiences/material-fx/client/operations_gen.rs"]
mod operations_gen;

use std::path::Path;

const MANIFEST: &str = "../../examples/experiences/material-fx/experience.toml";
const GENERATOR: &str = "../../examples/experiences/material-fx/client/operations_gen.rs";

fn main() {
    println!("cargo::rerun-if-changed={MANIFEST}");
    println!("cargo::rerun-if-changed={GENERATOR}");
    let out_dir = std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR");
    operations_gen::write(Path::new(MANIFEST), Path::new(&out_dir));
}
