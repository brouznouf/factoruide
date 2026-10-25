#!/bin/sh
# The last step before pushing, not run after every edit (while iterating: `cargo test --profile fast
# -p fg-route --test it <name>`, scripts/addon-test.sh, scripts/route-compare.sh).
# Run the CI checks locally: Rust (fmt, clippy, tests, cargo deny), frontend (tsc, quest input, oxlint, translations),
# addon (stylua, smoke test) and shell scripts. Tools that are not installed are skipped.
set -eu
cd "$(dirname "$0")/.."
has() { command -v "$1" >/dev/null 2>&1; }

cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --release
if has cargo-deny; then cargo deny --log-level error check; else echo "skip: cargo deny (cargo install cargo-deny)"; fi

(cd app && npm run build && npm test && npx oxlint --deny-warnings && npm run check:i18n)

npx -y @johnnymorganz/stylua-bin@2.5.2 --check addon
scripts/addon-test.sh >/dev/null

if has shellcheck; then shellcheck scripts/*.sh; else echo "skip: shellcheck"; fi
echo "all checks passed"
