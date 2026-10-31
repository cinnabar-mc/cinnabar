# Getting started

This guide takes a personal client mod from nothing to running in the client. If you want a mod with a server half, follow this guide first for the shape of a component, then [Paired mods](paired-mods.md) for the project tooling.

## Tools

You need a Cinnabar source checkout and its build prerequisites. Install the WebAssembly target once:

```sh
rustup target add wasm32-unknown-unknown
```

Guest code is ordinary Rust built for `wasm32-unknown-unknown`. The `mod-host` tool in the checkout packs a built module into a component and can probe one without starting the client.

## A new project

Start from the hello example in `examples/mods/hello`. It is a complete personal mod: a crate that depends on `mod-api`, implements the generated `Guest` trait, and exports itself as the `extension` world. The commands below build that example in place. `cinnabar-cxb mod new` scaffolds paired projects only; it does not create a personal-mod project.

```toml
[package]
name = "hello-mod"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
mod-api = { path = "../../../crates/mod-api" }

[lints]
workspace = true
```

To make your own mod, copy the directory, point the `mod-api` path at the checkout's `crates/mod-api`, and give the package your own name. The manifest above inherits values and lints from the checkout's workspace: add an in-tree copy to the root `workspace.members`, or, for a copy outside the checkout, replace the inherited package fields with explicit values and remove `[lints]`. Use your new package and artifact names in the build commands. The source, from `examples/mods/hello/src/lib.rs`:

```rust
use mod_api::bindings::{
    Guest,
    cinnabar::extension::{hud, input},
};

struct Hello;

impl Guest for Hello {
    /// Publishes the initial label without touching the client's game state.
    fn init() {
        let _ = hud::set_label("Hello from a Cinnabar mod. Press the demo key.");
    }

    /// Reacts to the local action without synthesizing gameplay input.
    fn frame() {
        if input::demo_pressed() {
            let _ = hud::set_label("Hello! The mod received your keybind.");
        }
    }
}

mod_api::bindings::export!(Hello with_types_in mod_api::bindings);
```

Two things to notice. `init` and `frame` are the only callbacks the `extension` world requires; [How mods run](how-mods-run.md) explains when each runs. And the imports under `mod_api::bindings::cinnabar::extension` are generated from the same WIT the host enforces, so a call that is not in the API does not compile.

State lives in your own statics. The examples keep it in `thread_local!` cells, as `examples/mods/time-changer/src/lib.rs` does for its current mode.

## Build and pack

```sh
cargo build -p hello-mod --target wasm32-unknown-unknown --locked
cargo run -p mod-host --bin mod-host --locked -- pack \
  target/wasm32-unknown-unknown/debug/hello_mod.wasm /tmp/cinnabar-hello.wasm
cargo run -p mod-host --bin mod-host --locked -- probe /tmp/cinnabar-hello.wasm
```

`pack` wraps the core module as a component. `probe` is a smoke test aimed at the sample mods: it loads the component in the host, runs `init` and two frames, and checks that the sample publishes a label and reacts to the demo key, so it suits a hello-style mod rather than an arbitrary one. Use `--release` on the build for a mod you will run for real.

## Install and reload

Import the packed component in Settings → Mods, review its permissions and enable it. [Grants](grants.md) explains what the player is being asked for and how a mod requests capabilities.

Reload rereads the installed path, so to update a mod, replace that file atomically and reload. An invalid replacement keeps the previous instance running. A successful reload resets guest memory — your statics start again — and preserves the companion settings described in [Settings and storage](settings-and-storage.md).

For development you can skip the library: set `CINNABAR_MOD_COMPONENT` to an absolute component path. `CINNABAR_MOD_PACKAGE` takes precedence over it, and `CINNABAR_MOD_SET` takes precedence over both: set it to the path of a JSON file containing `{"version":1,"mods":[{"component":"/absolute/mod.wasm","grants":{}}]}`. Each entry names either a `component` or a `package` and has its own grants. Registered enabled mods run before environment-selected ones. The client still needs its normal runtime carriers either way.

At most `MAX_LOADED_MODS` (4) local mods run at once.

## The example mods

| Example | Shows |
| --- | --- |
| `examples/mods/hello` | The smallest `extension` mod: a HUD label and the demo key. |
| `examples/mods/time-changer` | `environment::set_time_override` cycling local visual time, with state in a `thread_local!`. |
| `examples/mods/render-sample` | A WGSL post-process pass and a world-space decal, updated per frame from `gameplay::read_frame`. |
| `examples/mods/screen-probe` | A `player-mod` package: `mod.toml`, overlay and view templates, declared keys and actions, and every event callback. |
| `examples/mods/cosmic` | A paired mod: server guest and client part in one `.cmod`. See [Paired mods](paired-mods.md). |

Next: [How mods run](how-mods-run.md) for the callback and budget rules every guide after this one assumes.
