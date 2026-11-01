//! Bindings for the action filter, every event handler, and ordinary callbacks.
wit_bindgen::generate!({
    path: [
        "../experience-sdk/wit/client/deps/server-experience",
        "../experience-sdk/wit/session",
        "wit",
    ],
    world: "cinnabar:extension/handled-player-mod@0.1.0",
    generate_all,
    pub_export_macro: true,
    export_macro_name: "export_handled_player_mod",
});
