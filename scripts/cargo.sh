#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ -d "$root/.build/cargo/registry" ]; then export CARGO_HOME="$root/.build/cargo"; fi
exec cargo "$@"
