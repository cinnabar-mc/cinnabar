//! Writes the declarations of the showcase's `experience.toml` for `include_declarations!`, and
//! the control channel's operations as [`operations_gen::write`] numbers them.

#[path = "operations_gen.rs"]
mod operations_gen;

use std::path::Path;

const MANIFEST: &str = "../experience.toml";

fn main() {
    println!("cargo::rerun-if-changed=operations_gen.rs");
    experience_sdk::declarations::generate(MANIFEST);
    let out_dir = std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR");
    operations_gen::write(Path::new(MANIFEST), Path::new(&out_dir));
}
