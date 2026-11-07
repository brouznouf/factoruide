#!/bin/sh
# Publish a new version: tag the pushed main commit vX.Y.Z with the release notes and push the
# tag. The CI (.github/workflows/release.yml) builds the Windows installer, the macOS apps and the
# Linux app and publishes them (installed apps offer the update at startup, showing the notes),
# deploys the web app, and publishes each addon whose version is the tag's on CurseForge and Wago.
# An addon changed since the last release gets the new version first (a commit pushed on main),
# its CHANGELOG.md's Unreleased section becoming the version's (its changelog on CurseForge and
# Wago; the release stops when it is empty): the apps built from the tag then ask for it; an
# unchanged one keeps its version and is not published again.
#
#   scripts/release.sh 0.2.0 "Short notes (shown in the app): a link to CHANGELOG.md"
#   scripts/release.sh 0.2.0            # writes the notes in $EDITOR
set -eu
cd "$(dirname "$0")/.."

version=${1:-}
case $version in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *)
        echo "usage: $0 X.Y.Z [notes]" >&2
        exit 2
        ;;
esac
tag="v$version"

[ "$(git branch --show-current)" = main ] || { echo "not on main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "uncommitted changes" >&2; exit 1; }
git fetch -q origin main --tags
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "main differs from origin/main: push or pull first" >&2; exit 1; }
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    echo "$tag already exists" >&2
    exit 1
fi

# The addons changed since the last release, each with what changes for players.
last=$(git describe --tags --abbrev=0 --match 'v*.*.*' 2>/dev/null || true)
changed=
for addon in Factoruide_Profile Factoruide_Guide; do
    if [ -z "$last" ] || ! git diff --quiet "$last" HEAD -- "addon/$addon"; then
        if ! scripts/addon-changes.sh "$addon" "$version" >/dev/null && ! scripts/addon-changes.sh "$addon" >/dev/null; then
            echo "$addon changed since ${last:-the start}: write what changes for players under '## Unreleased' in addon/$addon/CHANGELOG.md" >&2
            exit 1
        fi
        changed="$changed $addon"
    fi
done

# The guide this release's apps make, kept for the addon's tests (a newer addon must read it).
# Not released yet: made again from what is committed now.
rm -f "addon/tests/guides/$version.lua"
scripts/guide-fixture.sh "$version"
if [ -n "$(git status --porcelain addon/tests/guides)" ]; then
    git add addon/tests/guides
    git commit -q -m "Addon tests: the guide of $version" -- addon/tests/guides
    git push -q origin main
fi

# The changed addons take this version, their Unreleased changes with it.
for addon in $changed; do
    sed -i "s/^## Version: .*/## Version: $version/" "addon/$addon/$addon.toc"
    if ! scripts/addon-changes.sh "$addon" "$version" >/dev/null; then
        sed -i "0,/^## Unreleased\$/s//## Unreleased\n\n## $version ($(date -u +%F))/" "addon/$addon/CHANGELOG.md"
    fi
done
# The published addons only: a local data addon (addon/Factoruide_Data, written by `fg route`) is not git's.
published="addon/Factoruide_Profile addon/Factoruide_Guide"
# shellcheck disable=SC2086
if [ -n "$(git status --porcelain $published)" ]; then
    # shellcheck disable=SC2086
    git commit -q -m "Addons $version:$changed" -- $published
    git push -q origin main
    echo "addons $version:$changed"
fi

if [ $# -ge 2 ]; then
    git tag -a "$tag" -m "$2"
else
    git tag -a "$tag"
fi
git push origin "$tag"
echo "$tag pushed: https://github.com/brouznouf/factoruide/actions/workflows/release.yml"
