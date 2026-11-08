#!/usr/bin/env bash
set -euo pipefail

cargo build --release
patchelf --remove-rpath target/release/filem
