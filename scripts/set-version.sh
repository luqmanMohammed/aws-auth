#!/bin/sh
# Sets the [package] version in Cargo.toml and syncs Cargo.lock.
# Called by semantic-release during prepare; also runnable by hand.
set -eu

if [ $# -ne 1 ]; then
    echo "usage: $0 <version>" >&2
    exit 2
fi
version=$1

# Only the first `version = ` line, which is the one in [package]. Dependency versions
# further down the file must not be touched. POSIX awk, so this behaves the same
# everywhere rather than relying on a GNU sed range.
awk -v v="$version" '
    /^version = / && !done { print "version = \"" v "\""; done = 1; next }
    { print }
' Cargo.toml >Cargo.toml.next
mv Cargo.toml.next Cargo.toml

# Fail loudly rather than tagging a release whose manifest was not actually updated.
grep -q "^version = \"$version\"\$" Cargo.toml || {
    echo "set-version: Cargo.toml was not updated to $version" >&2
    exit 1
}

# Updates only workspace members, leaving dependency resolutions alone.
cargo update --workspace --quiet

# Proves the manifest and the lockfile agree, which `--locked` builds depend on.
cargo metadata --locked --no-deps --format-version 1 >/dev/null || {
    echo "set-version: Cargo.lock is out of sync with Cargo.toml" >&2
    exit 1
}

echo "set-version: Cargo.toml and Cargo.lock set to $version"
