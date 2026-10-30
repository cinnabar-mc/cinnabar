# iOS (experimental)

The iOS client builds the shared Rust client for arm64 iPhones and iPads on iOS 17+,
rendering with Metal. Its [iOS host](../packaging/ios/client/Cargo.toml) enables native Rust
networking. The IPA carries no helper executables or preparation kit, so compile game assets
on your computer before sideloading. It is not on the App Store.

Earlier iPhone 18 Pro Max runs covered menus, sign-in, server joins and shader packs, with
limited touch testing. Physical-device acceptance of the current 1.26.60 networking path
remains pending.

## Get the IPA

The iOS workflow builds `Cinnabar-<version>-unsigned.ipa` and attaches it to the run as
`cinnabar-ios-unsigned`. To get one, push a branch named `ios/<anything>` to your fork of the
repository (enable Actions on the fork first) or run the iOS workflow by hand, then download it
with `gh run download <run id> -R <you>/cinnabar -n cinnabar-ios-unsigned`. On a Mac with
Xcode 26, the pinned Rust toolchain, the iOS target (`rustup target add aarch64-apple-ios`), and librsvg or
ImageMagick, build it locally instead:

```sh
bash packaging/ios/build-ipa.sh
```

The [build script](../packaging/ios/build-ipa.sh) compiles the `cinnabar-ios` Rust host and
stages shared resources. The IPA lands in `target/ios/`. Pinned values (bundle id,
deployment target, Xcode major) live in [runtime.json](../packaging/ios/runtime.json).

## Add your assets

The IPA carries no Mojang-derived files. Compile the carriers on your own computer, then copy
them and the vanilla resource pack into the IPA:

```sh
make assets
python packaging/ios/add-assets.py path/to/Cinnabar-<version>-unsigned.ipa
```

This writes `Cinnabar-<version>-assets.ipa` beside the input (`--out` picks another path,
`--local` another `.local` directory). The assets stay on your machine and device.

## Sideload from Windows

Either tool signs the IPA with your Apple ID. A free Apple ID signs it for 7 days; sideload it
again to renew. Either way, after the first install enable **Settings > Privacy & Security >
Developer Mode**, restart when asked, and if iOS says the developer is untrusted, trust it in
**Settings > General > VPN & Device Management**.

- [iloader](https://iloader.app/) works with iTunes from the Microsoft Store: connect the device
  by USB, trust the computer, sign in, and choose **Import IPA** with the `-assets` IPA.
- [Sideloadly](https://sideloadly.io/) needs iTunes and iCloud from Apple's website instead of
  their Microsoft Store versions; open the `-assets` IPA in it and sign in.

iOS stops an app that passes its memory limit, about 3.3 GB on a 12 GB iPhone, and servers that
send large resource packs can pass it while the world loads. The App ID's **Increased Memory
Limit** capability raises the limit. iloader's Import IPA leaves it off, but once it is on for
the App ID, later installs keep it; `isideload`'s `install_app` turns it on when its
`increased_memory_limit` argument is true.

## Logs and configuration

The app's `Documents` folder appears in the Files app under **On My iPhone > Cinnabar**. Logs are
in `data/logs`, settings and `servers.json` in `config`. Include the newest log when reporting a
problem. The build includes the experimental Java shader-pack renderer: drop a pack's zip into
`data/shaderpacks` and pick it in the Shaders tab. Packs render at the screen's native 3×
resolution, so a heavy pack is GPU-bound on a phone; its lower quality profiles cost less.

## Controls

The same on-screen touch controls as on Android: the stick moves, dragging elsewhere turns,
and the buttons jump, sneak, sprint, attack, use, pause and open the inventory. Menu text fields
and chat ask iOS for its on-screen keyboard; nothing yet moves a field the keyboard covers. A Bluetooth
keyboard and mouse work only as far as winit's UIKit backend delivers their input.

## Not supported

- Local worlds, mods and anything that needs JIT or a second process.
- Preparing assets on the device: inject them with `add-assets.py` before sideloading.
- The crash monitor and native crash dumps; Rust panics still write crash folders.
- Clipboard and opening links. Sign in with the device code on another device.
