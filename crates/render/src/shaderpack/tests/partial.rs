//! Keeps supported passes runnable when one program fails translation.

use super::*;

#[test]
fn a_failed_program_leaves_other_programs_runnable() {
    let pack = pack(&[
        ("final.vsh", PASS_VERTEX),
        ("final.fsh", FILL),
        ("composite.vsh", PASS_VERTEX),
        (
            "composite.fsh",
            "#version 120\nvoid main() { gl_FragData[0] = undefined_name; }\n",
        ),
    ]);
    let translation = pack.translate(&translate_config(&pack, JAVA_DIMENSIONS[0]));
    let (runnable, _) = runnable_programs(&translation).expect("program failures are isolated");
    assert_eq!(
        runnable
            .iter()
            .map(|p| p.program.name.as_str())
            .collect::<Vec<_>>(),
        ["final"]
    );
    assert!(
        translation.programs[0]
            .all_diagnostics()
            .any(|d| d.message.contains("undefined_name"))
    );
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: GPU adapter for partial shader pack loading");
        return;
    };
    let loaded = build(
        &pack,
        "fixture",
        JAVA_DIMENSIONS[0],
        &Default::default(),
        None,
        &gpu,
        &Default::default(),
        &Default::default(),
    )
    .expect("the supported final pass keeps the pack loadable");
    assert!(loaded.warnings.contains("composite:") && loaded.warnings.contains("undefined_name"));
    assert!(
        loaded
            .programs
            .iter()
            .any(|program| program.name == "final")
    );
}
