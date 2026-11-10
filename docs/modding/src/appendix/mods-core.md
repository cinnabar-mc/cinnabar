# Shared mod list (Mods Core)

[Mods Core](https://github.com/Emilchill/mods-core) is a community personal mod, not part
of Cinnabar. It puts every mod that supports it in one list: the player presses F10 (or taps
its **Mods** touch button), sees each mod's name, version and description, and clicks one to
open that mod's menu. Mods that use it need no default menu key, so they do not compete for
keys with each other.

It is built only on the [events](../reference/events.md) cue channel, which needs no grant,
and the mod's own [panel](../reference/panel.md), which needs the `controls` grant.

## Joining the list

Copy [`mods_core.rs`](https://github.com/Emilchill/mods-core/blob/main/mods/modscore/src/mods_core.rs)
into the mod and call its `Link` in every frame callback:

```rust
thread_local! {
    static CORE: std::cell::RefCell<mods_core::Link> = std::cell::RefCell::new(mods_core::Link::new(
        "my-mod",                  // slug: 1-21 bytes of a-z, 0-9 and -
        "My Mod",
        env!("CARGO_PKG_VERSION"),
        "What the mod does, in one or two short lines.",
    ));
}

// In `frame`, after reading controls:
let event = CORE.with_borrow_mut(|link| link.frame(controls.panel_open, || whole_panel_json()));
```

`whole_panel_json` returns the JSON the mod would give `panel.set-content`; `Link` calls it
only when the menu opens. Give the panel a `toggle_key` no keyboard sends, such as `F24`,
and do not use F10. A key the player sets can call `link.toggle(controls.panel_open)`.

## Why the panel comes and goes

Only one mod menu shows at a time: the panel of the mod last selected with its toggle key, or,
when that mod has no panel, the first mod in load order that has one. A mod cannot select
itself. Mods in the list therefore keep no panel while their menu is closed:

1. A click in the list emits `mcore.open.<slug>`, and Mods Core removes its own panel.
2. The chosen mod sets its whole panel and opens it, so it is the only menu.
3. While its menu is open it emits `mcore.busy.<slug>` every frame.
4. When the menu closes it removes its panel, and Mods Core restores its own once no mod has
   been busy for a moment.

`controls.panel-open` reports the shown menu, whichever mod owns it; `Link::has_panel`
tells a mod whether that menu is its own.

## Cues

| Cue | Sent by | Values |
| --- | --- | --- |
| `mcore.t.<slug>` | each listed mod, one chunk every other frame | `[index + 256 * count, checksum, d0..d5]` |
| `mcore.open.<slug>` | Mods Core on a click, or a mod for its own key | none |
| `mcore.busy.<slug>` | a mod, every frame its menu is open | none |

The info text is UTF-8 `name`, U+001F, `version`, U+001F, `description`, at most 252 bytes,
sent in 18-byte chunks. Each `d` packs three bytes as `b0 * 65536 + b1 * 256 + b2`, which an
`f32` holds exactly. The checksum starts at 7 and folds each byte as
`sum = (sum * 31 + byte) & 0xFFFF`. Mods Core forgets a mod it has not heard from for about
four seconds. The [Mods Core README](https://github.com/Emilchill/mods-core#readme) has the
full guide and a complete example.
