#!/bin/sh
# Builds the CI tab to WebAssembly and puts it where the manifest's `native`
# says: tabs/ci.wasm, which is committed, since Hangar runs the module a kit
# ships and never builds one. Run it from anywhere, and commit the module with
# the source that made it.
#
# The toolchain is native/rust-toolchain.toml's (rustup fetches it, with the
# wasm32-unknown-unknown target), and the paths a module remembers are made the
# same on every machine, so that CI can tell a committed module is stale by
# building it again: `scripts/build-native.sh && git diff --exit-code tabs/ci.wasm`.
set -eu
kit="$(cd "$(dirname "$0")/.." && pwd)"
cd "$kit/native"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$kit=/kit --remap-path-prefix=$cargo_home=/cargo"
cargo build --locked --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/kit_ci_tab.wasm "$kit/tabs/ci.wasm"
chmod 644 "$kit/tabs/ci.wasm"
echo "$kit/tabs/ci.wasm"
