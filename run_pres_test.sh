#!/bin/bash
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
# Use WSL-side target dir to avoid Windows fs permission issues on
# .fingerprint files. The source tree lives on /mnt/c, but build
# artifacts go to ~/.cache/altivex_target.
export CARGO_TARGET_DIR="$HOME/.cache/altivex_target"
# Cap lints to allow + force short error format. Workaround for a
# rustc 1.95 ICE in annotate_snippets renderer triggered by
# sqlx-postgres 0.7.4 future-breakage diagnostic output.
export RUSTFLAGS="--cap-lints=allow"
export CARGO_TERM_COLOR=never
mkdir -p "$CARGO_TARGET_DIR"
cd /mnt/c/Users/USER/Documents/ALTIVEX/altivex_backend
cargo --version
echo "--- target: $CARGO_TARGET_DIR ---"
"$@"
