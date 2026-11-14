# Live-heap attribution

The client's `heap-profile` feature (Linux with glibc only) swaps in an allocator that records
which call stacks hold live memory. It records every allocation of at least 64 KiB exactly and a
byte-weighted sample of smaller ones (about one per 256 KiB allocated per thread, each counted as
256 KiB), and forgets each one when it is freed. Allocation-heavy work runs several times slower.

Run the client on a private headless Wayland compositor; Xvfb offers Vulkan no present mode.

```sh
cargo build -p cinnabar --features heap-profile
mkdir -m 700 /tmp/heap-xdg
XDG_RUNTIME_DIR=/tmp/heap-xdg weston --backend=headless --renderer=gl --socket=heap-wl &
env -u DISPLAY XDG_RUNTIME_DIR=/tmp/heap-xdg WAYLAND_DISPLAY=heap-wl \
  CINNABAR_HEAP_PROFILE=/tmp/heap CINNABAR_HEAP_PROFILE_EVERY=15 \
  CINNABAR_USER_ROOT=/tmp/heap-user target/debug/cinnabar
python3 tools/heap-profile/symbolize.py target/debug/cinnabar /tmp/heap/heap-<pid>-004.txt
```

Each process writes `heap-<pid>-<n>.txt`; the crash monitor is a second client process with its
own small dumps. A dump starts with the process's live bytes, glibc's `mallinfo2` totals and
`/proc/self` resident, proportional and swapped sizes, then lists call stacks by the live bytes
they hold. `symbolize.py` resolves frames with `addr2line`, attributes each stack to its first
frame outside the allocator and the standard library, and prints totals by crate and owner frame.
Workspace crates carry line tables in the `dev` profile; dependency frames resolve to symbol
names only. Keep dumps outside the checkout.
