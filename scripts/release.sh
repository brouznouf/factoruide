#!/bin/sh
# Publish a new version of the app: tag the pushed main commit vX.Y.Z with the release notes and
# push the tag. The CI (.github/workflows/release.yml) builds the Windows installer and publishes
# it; installed apps offer the update at startup, showing the notes.
#
#   scripts/release.sh 0.2.0 "What changed (shown in the app)"
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

if [ $# -ge 2 ]; then
    git tag -a "$tag" -m "$2"
else
    git tag -a "$tag"
fi
git push origin "$tag"
echo "$tag pushed: https://github.com/brouznouf/factoruide/actions/workflows/release.yml"
