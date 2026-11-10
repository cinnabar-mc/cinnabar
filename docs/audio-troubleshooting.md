# No sound

When Cinnabar can't open your sound output, it shows a **No audio** toast and keeps retrying in the
background. Sounds play as soon as an output opens. The client log names the exact error: open the
newest `client-*.log` in the `logs` folder of Cinnabar's data directory and search for
`audio output unavailable`. On Linux that folder is `~/.local/share/cinnabar/logs/`.

## Linux

Cinnabar plays sound through ALSA's `default` device. On desktops that run PipeWire or PulseAudio,
that device only works when the sound server's ALSA plugin is installed. Without it, ALSA tries
the sound card directly, which the sound server already holds, so the game stays silent while
other apps play normally.

1. Find your sound server:

   ```sh
   pactl info | grep "Server Name"
   ```

   `PulseAudio (on PipeWire …)` means PipeWire. Plain `pulseaudio` means PulseAudio.

2. Install the matching ALSA plugin:

   | Distribution | PipeWire | PulseAudio |
   | --- | --- | --- |
   | Debian, Ubuntu, Mint, Pop!_OS | `sudo apt install pipewire-alsa` | `sudo apt install libasound2-plugins` |
   | Fedora | `sudo dnf install pipewire-alsa` | `sudo dnf install alsa-plugins-pulseaudio` |
   | Arch, Manjaro, EndeavourOS | `sudo pacman -S pipewire-alsa` | `sudo pacman -S pulseaudio-alsa` |
   | openSUSE | `sudo zypper install pipewire-alsa` | `sudo zypper install alsa-plugins-pulse` |

3. Check that ALSA's default device plays sound. `speaker-test` comes with `alsa-utils`:

   ```sh
   speaker-test -c 2 -t wav -l 1
   ```

   You should hear "front left" and "front right". If you do, restart Cinnabar.

If `speaker-test` stays silent after installing the plugin, log out and back in, or restart the
sound server with `systemctl --user restart pipewire pipewire-pulse wireplumber` (PipeWire) or
`pulseaudio -k` (PulseAudio). A custom `~/.asoundrc` or `/etc/asound.conf` can also redirect
`default` to a card that doesn't exist; move it aside and try again.

## Windows

1. Open **Settings > System > Sound**.
2. Under **Output**, choose a device. If none is listed, plug in headphones or speakers, or check
   **All sound devices** for a disabled one and allow it.
3. Cinnabar picks up the new default output on its own within a second.

If another app is using the device in exclusive mode, close that app, or turn off **Allow
applications to take exclusive control of this device** in the device's advanced properties.

## macOS

1. Open **System Settings > Sound**.
2. Under **Output**, choose a device.
3. Cinnabar picks up the new default output on its own within a second.

## Still no sound

Ask in the Cinnabar Discord and attach the newest `client-*.log`.
