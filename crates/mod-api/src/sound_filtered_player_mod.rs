//! Explicit opt-in bindings for cancellable sound playback observations.
#![allow(
    clippy::too_many_arguments,
    reason = "Canonical ABI flattens screen records"
)]
wit_bindgen::generate!({
    path: [
        "../experience-sdk/wit/client/deps/server-experience",
        "../experience-sdk/wit/session",
        "wit",
    ],
    world: "cinnabar:extension/sound-filtered-player-mod@0.1.0",
    generate_all,
    pub_export_macro: true,
    export_macro_name: "export_sound_filtered_player_mod",
});
