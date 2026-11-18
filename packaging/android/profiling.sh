#!/usr/bin/env bash
# Sourced before Cargo runs and before the Android manifest is rendered.

# Enable shell profiling with matching native symbols while keeping release optimizations.
configure_android_profiling() {
    profileable="${CINNABAR_ANDROID_PROFILEABLE:-false}"
    case "$profileable" in
        false) return 0 ;;
        true) ;;
        *) echo 'CINNABAR_ANDROID_PROFILEABLE must be true or false' >&2; return 1 ;;
    esac
    if [[ "$profile" != release ]]; then
        echo 'profileable APKs require CINNABAR_ANDROID_PROFILE=release' >&2
        return 1
    fi
    export CARGO_PROFILE_RELEASE_DEBUG=line-tables-only
    export CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO=off
    export CARGO_PROFILE_RELEASE_STRIP=none
    export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,--build-id=sha1"
}
