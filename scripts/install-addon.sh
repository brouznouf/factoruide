#!/bin/sh
# Copy the addon into the WoW client. After a change: /reload in game
# (a full client restart is needed when files are added or the .toc changes).
# Config.lua and Routes.lua are written by the Factoruide app: existing ones are kept.
set -eu
wow_dir=${FG_WOW_DIR:-"/mnt/g/Jeux/World of Warcraft"}
client=${FG_CLIENT:-_classic_beta_}
dest="$wow_dir/$client/Interface/AddOns/Factoruide"
src="$(dirname "$0")/../addon/Factoruide"
# The addon was called BrouzQuest: remove it (the game would load both) and carry its
# SavedVariables over once.
rm -rf "$wow_dir/$client/Interface/AddOns/BrouzQuest"
for old in "$wow_dir/$client"/WTF/Account/*/SavedVariables/BrouzQuest.lua; do
    [ -e "$old" ] || continue
    new="$(dirname "$old")/Factoruide.lua"
    [ -e "$new" ] || { sed 's/BrouzQuestDB/FactoruideDB/g' "$old" > "$new" && mv "$old" "$old.migrated"; }
done
mkdir -p "$dest"
for f in "$dest"/*.lua "$dest"/*.toc "$dest"/*.xml; do
    [ -e "$f" ] || continue
    case "$(basename "$f")" in Config.lua | Routes.lua) continue ;; esac
    rm -f "$f"
done
for f in "$src"/*.lua "$src"/*.toc "$src"/*.xml; do
    name=$(basename "$f")
    if { [ "$name" = "Config.lua" ] || [ "$name" = "Routes.lua" ]; } && [ -e "$dest/$name" ]; then
        continue
    fi
    cp "$f" "$dest/"
done
# Textures.
mkdir -p "$dest/Media"
cp "$src"/Media/*.tga "$dest/Media/"
echo "installed into $dest"
