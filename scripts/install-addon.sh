#!/bin/sh
# Copy the addon (Factoruide_Profile and Factoruide_Guide) into the WoW client. After a change:
# /reload in game (a full client restart is needed when files are added or a .toc changes).
# (As an addon site would; the guides of `fg route`, addon/Factoruide_Data, with FG_INSTALL_DATA=1
# only. Existing Config.lua, Routes.lua, Services.lua and Sent.lua written by older apps are kept.)
set -eu
wow_dir=${FG_WOW_DIR:-"/mnt/g/Jeux/World of Warcraft"}
client=${FG_CLIENT:-$([ -d "$wow_dir/_forever_" ] && echo _forever_ || echo _classic_beta_)}
addons="$wow_dir/$client/Interface/AddOns"
repo="$(dirname "$0")/../addon"
# The addon was called BrouzQuest, then was one addon, Factoruide: remove them (the game would
# load them too), keep the app's files of the old one, and carry their SavedVariables over.
rm -rf "$addons/BrouzQuest"
for old in "$wow_dir/$client"/WTF/Account/*/SavedVariables/BrouzQuest.lua; do
    [ -e "$old" ] || continue
    new="$(dirname "$old")/Factoruide.lua"
    [ -e "$new" ] || { sed 's/BrouzQuestDB/FactoruideDB/g' "$old" > "$new" && mv "$old" "$old.migrated"; }
done
if [ -d "$addons/Factoruide" ]; then
    mkdir -p "$addons/Factoruide_Guide" "$addons/Factoruide_Profile"
    # Written for the single addon's namespace: the guide part uses the shared one.
    for name in Config.lua Routes.lua; do
        [ -e "$addons/Factoruide/$name" ] && [ ! -e "$addons/Factoruide_Guide/$name" ] \
            && sed 's/^local _, FG = \.\.\.$/local FG = Factoruide/' "$addons/Factoruide/$name" > "$addons/Factoruide_Guide/$name"
    done
    [ -e "$addons/Factoruide/Sent.lua" ] && [ ! -e "$addons/Factoruide_Profile/Sent.lua" ] \
        && cp "$addons/Factoruide/Sent.lua" "$addons/Factoruide_Profile/"
    rm -rf "$addons/Factoruide"
fi
# The old addon's SavedVariables, unless the new one has newer ones.
for old in "$wow_dir/$client"/WTF/Account/*/SavedVariables/Factoruide.lua; do
    [ -e "$old" ] || continue
    new="$(dirname "$old")/Factoruide_Profile.lua"
    if [ ! -e "$new" ] || [ -n "$(find "$old" -newer "$new")" ]; then
        cp "$old" "$new" && mv "$old" "$old.migrated"
    fi
done
for addon in Factoruide_Profile Factoruide_Guide; do
    dest="$addons/$addon"
    src="$repo/$addon"
    mkdir -p "$dest"
    for f in "$dest"/*.lua "$dest"/*.toc "$dest"/*.xml; do
        [ -e "$f" ] || continue
        case "$(basename "$f")" in Config.lua | Routes.lua | Services.lua | Sent.lua) continue ;; esac
        rm -f "$f"
    done
    for f in "$src"/*.lua "$src"/*.toc "$src"/*.xml; do
        [ -e "$f" ] || continue
        name=$(basename "$f")
        case "$name" in Sent.lua) continue ;; esac
        if { [ "$name" = "Config.lua" ] || [ "$name" = "Routes.lua" ] || [ "$name" = "Services.lua" ]; } \
            && [ -e "$dest/$name" ]; then
            continue
        fi
        cp "$f" "$dest/"
    done
    # Libraries.
    if [ -d "$src/Libs" ]; then
        rm -rf "$dest/Libs"
        cp -r "$src/Libs" "$dest/Libs"
    fi
    # Translations.
    if [ -d "$src/Locales" ]; then
        rm -rf "$dest/Locales"
        mkdir -p "$dest/Locales"
        cp "$src"/Locales/*.lua "$dest/Locales/"
    fi
    # Textures.
    if [ -d "$src/Media" ]; then
        mkdir -p "$dest/Media"
        cp "$src"/Media/*.tga "$dest/Media/"
    fi
    echo "installed into $dest"
done
# The guides planned with `fg route` (the data addon it writes), only when asked
# (FG_INSTALL_DATA=1): the game's Factoruide_Data holds the guides the app wrote, never replaced.
if [ "${FG_INSTALL_DATA:-}" = 1 ] && [ -f "$repo/Factoruide_Data/Data.lua" ]; then
    rm -rf "$addons/Factoruide_Data"
    cp -r "$repo/Factoruide_Data" "$addons/Factoruide_Data"
    echo "installed into $addons/Factoruide_Data"
fi
