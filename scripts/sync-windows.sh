#!/bin/sh
# Mirror the project to a Windows folder to build/run the desktop app natively there.
# Build artifacts are excluded; the bundled databases (data/*.sqlite.zst) are copied.
set -eu
dest=${FG_WINDOWS_DIR:-/mnt/g/Factoruide/App}
src="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$dest"
rsync -a --delete \
    --exclude target/ --exclude node_modules/ --exclude 'app/dist/' --exclude 'app/src-tauri/gen/' \
    --exclude '.git/' --exclude '/data/*.sqlite' --exclude '/routes/' --exclude '/debug/' \
    --exclude 'addon/Factoruide/Config.lua' --exclude 'addon/Factoruide/Routes.lua' \
    "$src/" "$dest/"
echo "synced to $dest"
