#![expect(clippy::print_stdout, reason = "the tool prints what it decodes")]

//! `material-bin summary <file>` lists a compiled material's buffers, uniforms, passes and
//! variant flags; `material-bin source <file> <pass> <stage> [FLAG=VALUE ...]` prints the
//! shader source of every variant of `pass` whose flags match. Read local game installs only;
//! the output is game data and must not be committed.

use anyhow::{Context, Result, bail};
use material_bin::{Material, decode};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (command, path) = match args.as_slice() {
        [command, path, ..] => (command.as_str(), path),
        _ => bail!("usage: material-bin summary|source <file> ..."),
    };
    let bytes = std::fs::read(path).with_context(|| format!("reading {path}"))?;
    let material = decode(&bytes).with_context(|| format!("decoding {path}"))?;
    match command {
        "summary" => summary(&material),
        "source" => {
            let [pass, stage, filters @ ..] = &args[2..] else {
                bail!("usage: material-bin source <file> <pass> <stage> [FLAG=VALUE ...]");
            };
            source(&material, pass, stage, filters)
        }
        other => bail!("unknown command {other}"),
    }
}

/// Prints the material's structure, one line per item.
fn summary(material: &Material) -> Result<()> {
    println!("material {} parent {:?}", material.name, material.parent);
    for buffer in &material.buffers {
        println!(
            "buffer {} register {} kind {} format {} default {:?}",
            buffer.name, buffer.register, buffer.kind, buffer.format, buffer.default_texture
        );
    }
    for uniform in &material.uniforms {
        println!(
            "uniform {} type {} count {} default {:?}",
            uniform.name, uniform.kind, uniform.count, uniform.default
        );
    }
    for pass in &material.passes {
        println!(
            "pass {} platforms {} fallback {:?} blend {:?} defaults {:?} variants {}",
            pass.name,
            pass.platforms,
            pass.fallback,
            pass.blend_mode,
            pass.default_flags,
            pass.variants.len()
        );
        for variant in &pass.variants {
            println!(
                "  variant supported {} flags {:?}",
                variant.supported, variant.flags
            );
        }
    }
    Ok(())
}

/// Prints the `stage` sources of `pass`'s variants matching every `FLAG=VALUE` filter.
fn source(material: &Material, pass: &str, stage: &str, filters: &[String]) -> Result<()> {
    let pass = material
        .passes
        .iter()
        .find(|candidate| candidate.name == pass)
        .with_context(|| format!("no pass {pass}"))?;
    let filters = filters
        .iter()
        .map(|filter| filter.split_once('=').context("filters are FLAG=VALUE"))
        .collect::<Result<Vec<_>>>()?;
    for variant in &pass.variants {
        if !filters
            .iter()
            .all(|(flag, value)| variant.flag(pass, flag) == Some(*value))
        {
            continue;
        }
        for shader in variant
            .shaders
            .iter()
            .filter(|shader| shader.stage == stage)
        {
            println!("// variant {:?} {}", variant.flags, shader.platform);
            println!("{}", shader.source()?);
        }
    }
    Ok(())
}
