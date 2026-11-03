#!/bin/sh
# Builds the CI tab to WebAssembly and puts it where the manifest's `entry`
# says: tabs/ci.wasm, which is committed, since Hangar runs the module a kit
# ships and never builds one. Run it from anywhere, and commit the module with
# the source that made it.
#
#   scripts/build-native.sh          build it
#   scripts/build-native.sh --check  build it, and fail if the committed one is not that
#
# The toolchain is native/rust-toolchain.toml's (rustup fetches it, with the
# wasm32-unknown-unknown target), and the paths a module remembers are made the
# same on every machine, so that a committed module can be told stale by
# building it again.
set -eu
kit="$(cd "$(dirname "$0")/.." && pwd)"
cd "$kit/native"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$kit=/kit --remap-path-prefix=$cargo_home=/cargo"
cargo build --locked --release --target wasm32-unknown-unknown
built="target/wasm32-unknown-unknown/release/kit_ci_tab.wasm"
if [ "${1:-}" = "--check" ]; then
  if cmp -s "$built" "$kit/tabs/ci.wasm"; then
    echo "tabs/ci.wasm is what native/ builds"
    exit 0
  fi
  echo "tabs/ci.wasm is stale: run scripts/build-native.sh and commit the result" >&2
  exit 1
fi
cp "$built" "$kit/tabs/ci.wasm"
chmod 644 "$kit/tabs/ci.wasm"
echo "$kit/tabs/ci.wasm"
