//! Live-heap attribution for developer runs on Linux (glibc).
//!
//! [`ProfilingAllocator`] records a call stack for every allocation of at least
//! [`LARGE_BYTES`] and for a byte-weighted sample of smaller ones (on average one per
//! [`SAMPLE_BYTES`] allocated on each thread, at exponentially distributed gaps, each standing
//! for that many bytes), and forgets
//! them when they are freed. [`start`] dumps what is still live, grouped by call stack, to
//! `$CINNABAR_HEAP_PROFILE/heap-<pid>-<n>.txt` every `$CINNABAR_HEAP_PROFILE_EVERY` seconds (15 by
//! default). Frames are written as `module+0xoffset`; `tools/heap-profile/symbolize.py`
//! resolves them against the binary. Recording slows allocation-heavy work several times over.

pub use crate::sampler::{LARGE_BYTES, SAMPLE_BYTES};

use std::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    path::{Path, PathBuf},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::Duration,
};

/// Return addresses kept per recorded allocation.
const DEPTH: usize = 40;
/// Independent locks over the live-allocation table, to keep frees from contending.
const SHARDS: usize = 64;
/// Sites written per dump, largest first.
const DUMPED_SITES: usize = 4000;

type Frames = [usize; DEPTH];

/// Forwards to `A` and records which call stacks hold live memory.
pub struct ProfilingAllocator<A>(pub A);

static ENABLED: AtomicBool = AtomicBool::new(false);
static LIVE_BYTES: AtomicI64 = AtomicI64::new(0);
static LIVE_COUNT: AtomicI64 = AtomicI64::new(0);
static ALLOCATED_TOTAL: AtomicU64 = AtomicU64::new(0);

/// A recorded allocation: the bytes it stands for and the site that made it.
#[derive(Clone, Copy)]
struct Live {
    weight: usize,
    site: u32,
}

static LIVE: [Mutex<Option<HashMap<usize, Live>>>; SHARDS] = [const { Mutex::new(None) }; SHARDS];

#[derive(Default)]
struct Sites {
    ids: HashMap<Frames, u32>,
    frames: Vec<Frames>,
}

static SITES: Mutex<Option<Sites>> = Mutex::new(None);

thread_local! {
    /// Set while this thread runs profiler code, whose own allocations are not recorded.
    static INSIDE: Cell<bool> = const { Cell::new(false) };
    /// Chooses this thread's sampled small allocations; seeded on first use.
    static SAMPLER: std::cell::RefCell<Option<crate::sampler::Sampler>> =
        const { std::cell::RefCell::new(None) };
}

/// Runs `work` with recording suppressed on this thread; `None` when the thread is already
/// inside the profiler or tearing down its locals.
fn guarded<T>(work: impl FnOnce() -> T) -> Option<T> {
    let entered = INSIDE
        .try_with(|inside| !inside.replace(true))
        .unwrap_or(false);
    if !entered {
        return None;
    }
    let result = work();
    let _ = INSIDE.try_with(|inside| inside.set(false));
    Some(result)
}

fn shard(pointer: usize) -> &'static Mutex<Option<HashMap<usize, Live>>> {
    &LIVE[(pointer >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15) % SHARDS]
}

/// The weight `size` is recorded with, or `None` when this allocation is not sampled.
fn weight(size: usize) -> Option<usize> {
    if size >= LARGE_BYTES {
        return Some(size);
    }
    SAMPLER
        .try_with(|sampler| {
            let mut sampler = sampler.try_borrow_mut().ok()?;
            let sampler = sampler.get_or_insert_with(|| {
                // Each thread draws its own sequence.
                let seed = SEEDS.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
                crate::sampler::Sampler::new(SAMPLE_BYTES, seed)
            });
            let points = sampler.sample(size);
            (points > 0).then(|| (points * sampler.mean()) as usize)
        })
        .ok()
        .flatten()
}

/// Seeds handed to each thread's sampler.
static SEEDS: AtomicU64 = AtomicU64::new(0x2545_F491_4F6C_DD1D);

fn track(pointer: *mut u8, size: usize) {
    if pointer.is_null() {
        return;
    }
    LIVE_BYTES.fetch_add(size as i64, Ordering::Relaxed);
    LIVE_COUNT.fetch_add(1, Ordering::Relaxed);
    ALLOCATED_TOTAL.fetch_add(size as u64, Ordering::Relaxed);
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some(weight) = weight(size) else {
        return;
    };
    guarded(|| {
        let mut frames = [0_usize; DEPTH];
        // SAFETY: the buffer holds DEPTH pointer-sized slots.
        let depth = unsafe { glibc::backtrace(frames.as_mut_ptr().cast(), DEPTH as i32) };
        frames[depth.max(0) as usize..].fill(0);
        let site = {
            let mut sites = SITES.lock().unwrap_or_else(PoisonError::into_inner);
            let sites = sites.get_or_insert_with(Sites::default);
            let next = sites.frames.len() as u32;
            *sites.ids.entry(frames).or_insert_with(|| {
                sites.frames.push(frames);
                next
            })
        };
        shard(pointer as usize)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(HashMap::new)
            .insert(pointer as usize, Live { weight, site });
    });
}

fn untrack(pointer: *mut u8, size: usize) {
    LIVE_BYTES.fetch_sub(size as i64, Ordering::Relaxed);
    LIVE_COUNT.fetch_sub(1, Ordering::Relaxed);
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    guarded(|| {
        if let Some(live) = shard(pointer as usize)
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            live.remove(&(pointer as usize));
        }
    });
}

fn inside() -> bool {
    INSIDE.try_with(Cell::get).unwrap_or(true)
}

// SAFETY: every call forwards the caller's layout and ownership to the wrapped allocator unchanged;
// the bookkeeping never touches the blocks themselves.
unsafe impl<A: GlobalAlloc> GlobalAlloc for ProfilingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        let pointer = unsafe { self.0.alloc(layout) };
        if !inside() {
            track(pointer, layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as above.
        let pointer = unsafe { self.0.alloc_zeroed(layout) };
        if !inside() {
            track(pointer, layout.size());
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let outside = !inside();
        if outside {
            untrack(pointer, layout.size());
        }
        // SAFETY: the caller owns `pointer` with `layout` and supplies a valid new size.
        let moved = unsafe { self.0.realloc(pointer, layout, size) };
        if outside {
            if moved.is_null() {
                track(pointer, layout.size());
            } else {
                track(moved, size);
            }
        }
        moved
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if !inside() {
            untrack(pointer, layout.size());
        }
        // SAFETY: the caller returns a live block with its original layout.
        unsafe { self.0.dealloc(pointer, layout) }
    }
}

/// Starts recording and, when `CINNABAR_HEAP_PROFILE` names a directory, a thread that dumps
/// the live heap there periodically. Call once, early in `main`.
pub fn start() {
    let Some(dir) = std::env::var_os("CINNABAR_HEAP_PROFILE").map(PathBuf::from) else {
        return;
    };
    let every = std::env::var("CINNABAR_HEAP_PROFILE_EVERY")
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .filter(|seconds| *seconds > 0)
        .map_or(Duration::from_secs(15), Duration::from_secs);
    // Loads the unwinder before any allocation can need it.
    guarded(|| {
        let mut frames = [0_usize; 4];
        // SAFETY: the buffer holds four pointer-sized slots.
        unsafe { glibc::backtrace(frames.as_mut_ptr().cast(), 4) };
    });
    ENABLED.store(true, Ordering::Relaxed);
    let _ = std::thread::Builder::new()
        .name("heap-profile".to_owned())
        .spawn(move || {
            let _ = INSIDE.try_with(|inside| inside.set(true));
            let _ = std::fs::create_dir_all(&dir);
            for index in 0..=u32::MAX {
                std::thread::sleep(every);
                let _ = dump(&dir.join(format!("heap-{}-{index:03}.txt", std::process::id())));
            }
        });
}

/// Writes the live heap grouped by call stack, with process and allocator totals.
fn dump(path: &Path) -> std::io::Result<()> {
    let mut by_site: HashMap<u32, (u64, u64)> = HashMap::new();
    for shard in &LIVE {
        let shard = shard.lock().unwrap_or_else(PoisonError::into_inner);
        for live in shard.iter().flat_map(HashMap::values) {
            let entry = by_site.entry(live.site).or_default();
            entry.0 += live.weight as u64;
            entry.1 += 1;
        }
    }
    let frames: Vec<Frames> = SITES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|sites| sites.frames.clone())
        .unwrap_or_default();
    let maps = Modules::read();
    let mut sites: Vec<_> = by_site.into_iter().collect();
    sites.sort_by_key(|site| std::cmp::Reverse(site.1.0));
    let recorded: u64 = sites.iter().map(|(_, (bytes, _))| bytes).sum();
    let mut out = String::new();
    // SAFETY: mallinfo2 only reads the allocator's statistics.
    let info = unsafe { glibc::mallinfo2() };
    let _ = writeln!(
        out,
        "live_bytes {} live_count {} allocated_total {} recorded_bytes {recorded}",
        LIVE_BYTES.load(Ordering::Relaxed),
        LIVE_COUNT.load(Ordering::Relaxed),
        ALLOCATED_TOTAL.load(Ordering::Relaxed),
    );
    let _ = writeln!(
        out,
        "mallinfo2 arena {} hblkhd {} uordblks {} fordblks {} keepcost {}",
        info.arena, info.hblkhd, info.uordblks, info.fordblks, info.keepcost
    );
    for file in ["/proc/self/smaps_rollup", "/proc/self/status"] {
        if let Ok(text) = std::fs::read_to_string(file) {
            for line in text.lines().filter(|line| {
                [
                    "Rss:",
                    "Pss:",
                    "Swap:",
                    "Anonymous:",
                    "VmRSS:",
                    "VmHWM:",
                    "RssAnon:",
                    "RssFile:",
                ]
                .iter()
                .any(|key| line.starts_with(key))
            }) {
                let _ = writeln!(out, "proc {line}");
            }
        }
    }
    for (site, (bytes, count)) in sites.iter().take(DUMPED_SITES) {
        let _ = writeln!(out, "site {bytes} {count}");
        if let Some(frames) = frames.get(*site as usize) {
            for frame in frames.iter().take_while(|frame| **frame != 0) {
                let _ = writeln!(out, "  {}", maps.describe(*frame));
            }
        }
    }
    std::fs::File::create(path)?.write_all(out.as_bytes())
}

/// Executable mappings from `/proc/self/maps`, to name frames by module and offset.
struct Modules(Vec<(usize, usize, usize, String)>);

impl Modules {
    fn read() -> Self {
        let text = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        let mut modules = Vec::new();
        let mut bases: HashMap<String, usize> = HashMap::new();
        for line in text.lines() {
            let mut fields = line.split_whitespace();
            let (Some(range), Some(perms), Some(offset)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let name = fields.nth(2).unwrap_or("").to_owned();
            let Some((start, end)) = range.split_once('-') else {
                continue;
            };
            let (Ok(start), Ok(end), Ok(offset)) = (
                usize::from_str_radix(start, 16),
                usize::from_str_radix(end, 16),
                usize::from_str_radix(offset, 16),
            ) else {
                continue;
            };
            if offset == 0 {
                bases.entry(name.clone()).or_insert(start);
            }
            if perms.contains('x') {
                let base = bases.get(&name).copied().unwrap_or(start - offset);
                modules.push((start, end, base, name));
            }
        }
        Self(modules)
    }

    /// `module+0xoffset` for a return address; the offset points into the calling instruction.
    fn describe(&self, address: usize) -> String {
        let at = address.saturating_sub(1);
        self.0
            .iter()
            .find(|(start, end, _, _)| (*start..*end).contains(&at))
            .map_or_else(
                || format!("?+{at:#x}"),
                |(_, _, base, name)| format!("{name}+{:#x}", at - base),
            )
    }
}

/// The glibc functions the profiler calls, declared here because this crate takes no
/// dependencies.
mod glibc {
    use std::ffi::{c_int, c_void};

    /// glibc's `struct mallinfo2`: allocator statistics in bytes.
    #[repr(C)]
    #[allow(
        dead_code,
        reason = "glibc fills the whole struct; the dump reports only some fields"
    )]
    pub(super) struct Mallinfo2 {
        /// Bytes the main arena took from the system with `brk`.
        pub arena: usize,
        pub ordblks: usize,
        pub smblks: usize,
        pub hblks: usize,
        /// Bytes in blocks taken with `mmap`.
        pub hblkhd: usize,
        pub usmblks: usize,
        pub fsmblks: usize,
        /// Bytes in allocated blocks.
        pub uordblks: usize,
        /// Bytes in free blocks the allocator keeps.
        pub fordblks: usize,
        /// Bytes at the top of the heap that could be returned to the system.
        pub keepcost: usize,
    }

    unsafe extern "C" {
        /// glibc `backtrace`: writes up to `size` return addresses and returns how many.
        pub(super) fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int;
        /// glibc `mallinfo2`, since glibc 2.33.
        pub(super) fn mallinfo2() -> Mallinfo2;
    }
}
