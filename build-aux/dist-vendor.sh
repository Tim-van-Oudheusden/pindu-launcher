#!/bin/sh
# `meson dist` script: vendors the crates Cargo.lock names into the release tarball, with
# the cargo config that points at them, so builders without network (RPM, DEB) build
# offline. crates/meson.build switches to offline builds when the config is present.
#
# Usage (from meson.add_dist_script): build-aux/dist-vendor.sh CARGO
set -eu

cargo=$1
cd "$MESON_DIST_ROOT"
mkdir -p .cargo
"$cargo" vendor --locked --versioned-dirs vendor >.cargo/config.toml
