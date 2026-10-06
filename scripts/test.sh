#!/bin/sh
set -eu
sh scripts/cargo.sh test --locked --manifest-path backend/Cargo.toml
python3 -m unittest discover -s tests -v
