#!/bin/sh
# Run the CI checks locally: Rust (fmt, clippy, tests, cargo deny), frontend (tsc, oxlint),
# addon (stylua, smoke test) and shell scripts. Tools that are not installed are skipped.
set -eu
cd "$(dirname "$0")/.."
has() { command -v "$1" >/dev/null 2>&1; }

cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
if has cargo-deny; then cargo deny --log-level error check; else echo "skip: cargo deny (cargo install cargo-deny)"; fi

(cd app && npm run build && npx oxlint --deny-warnings)

npx -y @johnnymorganz/stylua-bin@2.5.2 --check addon
if has luajit; then luajit addon/tests/smoke.lua >/dev/null; else echo "skip: addon smoke test (luajit)"; fi

if has shellcheck; then shellcheck scripts/*.sh; else echo "skip: shellcheck"; fi
echo "all checks passed"
