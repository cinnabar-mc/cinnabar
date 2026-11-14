#!/usr/bin/env python3
"""Resolve a `heap-profile` dump into readable call stacks and per-owner totals.

Usage: symbolize.py BINARY DUMP [--frames N] [--sites N]

Frames in BINARY are resolved with binutils `addr2line`; frames in shared libraries keep
their module name. Each site is attributed to its first frame outside the allocator, the
standard library and common container crates, and totals are printed by that owner and by
its crate.
"""

import argparse
import collections
import os
import re
import subprocess

SKIP = re.compile(
    r"^(\?\?|test_alloc::|alloc::|core::|std::|hashbrown::|__rust|_start|__libc|"
    r"<alloc::|<core::|<std::|<hashbrown::|<test_alloc::|smallvec::|<smallvec::|"
    r"bytes::|<bytes::|indexmap::|<indexmap::|serde::|<serde::|serde_json::|<serde_json::|"
    r"bincode::|<bincode::|postcard::|<postcard::|rkyv::|<rkyv::|<&|<\[|<\(|<T|"
    r"<\w+ as core::|<\w+ as alloc::|clone_from|from_iter|extend|collect|to_vec|to_owned)"
)


def parse(path):
    header, sites, current = [], [], None
    with open(path) as text:
        for line in text:
            if line.startswith("site "):
                _, size, count = line.split()
                current = {"bytes": int(size), "count": int(count), "frames": []}
                sites.append(current)
            elif line.startswith("  ") and current is not None:
                current["frames"].append(line.strip())
            else:
                header.append(line.rstrip())
    return header, sites


def resolve(binary, sites):
    exe = os.path.realpath(binary)
    offsets = sorted(
        {
            frame.rsplit("+", 1)[1]
            for site in sites
            for frame in site["frames"]
            if os.path.realpath(frame.rsplit("+", 1)[0]) == exe
        }
    )
    names = {}
    if offsets:
        output = subprocess.run(
            ["addr2line", "-f", "-C", "-i", "-a", "-e", exe, *offsets],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.splitlines()
        current = None
        lines = iter(output)
        for line in lines:
            if line.startswith("0x") and all(c in "0123456789abcdefx" for c in line):
                current = hex(int(line, 16))
                names[current] = []
                continue
            location = next(lines, "")
            names[current].append(f"{line}  [{location}]")
    for site in sites:
        resolved = []
        for frame in site["frames"]:
            module, offset = frame.rsplit("+", 1)
            if os.path.realpath(module) == exe:
                resolved.extend(names.get(hex(int(offset, 16)), [frame]))
            else:
                resolved.append(f"{os.path.basename(module)}+{offset}")
        site["resolved"] = resolved


def plumbing(frame):
    """Allocator, standard-library and profiler frames, which never own the memory."""
    name, _, location = frame.partition("  [")
    return (
        SKIP.match(name) is not None
        or "/rustc/" in location
        or "/library/" in location
        or "heap_profile.rs" in location
    )


def short(frame):
    name, _, location = frame.partition("  [")
    location = location.rstrip("]")
    for marker in ("/crates/", "/app/", "/tools/"):
        if marker in location:
            location = marker.strip("/") + "/" + location.split(marker, 1)[1]
            break
    else:
        location = os.path.basename(location)
    return f"{name}  [{location}]"


def owner(site):
    for frame in site["resolved"]:
        if not plumbing(frame):
            return short(frame)
    return site["resolved"][0] if site["resolved"] else "?"


def crate(name):
    path = re.search(r"\[(crates/[^/]+|app|tools/[^/]+)/", name)
    if path:
        return path.group(1)
    match = re.match(r"^<?([A-Za-z_][A-Za-z0-9_]*)::", name)
    return match.group(1) if match else name.split("+")[0]


def mib(size):
    return f"{size / 1048576:8.1f} MiB"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("dump")
    parser.add_argument("--frames", type=int, default=12)
    parser.add_argument("--sites", type=int, default=40)
    args = parser.parse_args()
    header, sites = parse(args.dump)
    resolve(args.binary, sites)
    print("\n".join(header))
    total = sum(site["bytes"] for site in sites)
    print(f"\nrecorded in dumped sites: {mib(total)}")
    by_crate, by_owner = collections.Counter(), collections.Counter()
    for site in sites:
        name = owner(site)
        by_owner[name] += site["bytes"]
        by_crate[crate(name)] += site["bytes"]
    print("\n== by crate")
    for name, size in by_crate.most_common(30):
        print(f"{mib(size)}  {name}")
    print("\n== by owner frame")
    for name, size in by_owner.most_common(50):
        print(f"{mib(size)}  {name}")
    print("\n== largest sites")
    for site in sites[: args.sites]:
        print(f"\n{mib(site['bytes'])} in {site['count']} allocations")
        shown = [short(f) for f in site["resolved"] if not plumbing(f)]
        for frame in shown[: args.frames]:
            print(f"    {frame}")


if __name__ == "__main__":
    main()
