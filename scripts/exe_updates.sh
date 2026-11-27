#!/usr/bin/env bash
# Makes the in-game updater's exe files (src/update.rs) from the release archives in dist/:
#   dist/longrange-<v>-<platform>-exe.zst          the whole exe, zstd
#   dist/longrange-<v>-<platform>-from-<pv>.zst    zstd --patch-from the previous release's exe (a few MB)
# The previous release is the newest published one on GitHub other than this tag (its archives are downloaded
# with `gh`). Assets need nothing: the updater fetches changed ones from the git tree of the tag.
#   scripts/exe_updates.sh v0.17.0         prints the files it made, one per line
# Needs zstd (1.4.5+), tar, unzip, gh (logged in, or GH_TOKEN).
set -euo pipefail
cd "$(dirname "$0")/.."
TAG="$1"
V="${TAG#v}"
command -v zstd >/dev/null || { echo "exe_updates: zstd not found" >&2; exit 1; }
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

PREV=$(gh release list --exclude-drafts --exclude-pre-releases --limit 20 --json tagName --jq '.[].tagName' 2>/dev/null | grep -vx "$TAG" | head -1 || true)
PV="${PREV#v}"

exe_of() { # <archive> <bundle folder> <exe name> <out>
    case "$1" in
        *.tar.gz) tar -xzf "$1" -O "$2/$3" > "$4" ;;
        *.zip) unzip -p "$1" "$2/$3" > "$4" ;;
    esac
    [ -s "$4" ]
}

for plat in linux-x86_64 windows-x86_64; do
    exe=longrange; [ "$plat" = windows-x86_64 ] && exe=longrange.exe
    arc=$(ls dist/longrange-"$V"-"$plat".tar.gz dist/longrange-"$V"-"$plat".zip 2>/dev/null | head -1 || true)
    [ -n "$arc" ] || continue
    exe_of "$arc" "longrange-$V-$plat" "$exe" "$TMP/new" || { echo "exe_updates: no $exe in $arc" >&2; exit 1; }
    out="dist/longrange-$V-$plat-exe.zst"
    zstd -q -f -19 -T0 --long=28 "$TMP/new" -o "$out"
    echo "$out"
    if [ -n "$PREV" ]; then
        rm -rf "$TMP/prev"; mkdir -p "$TMP/prev"
        if gh release download "$PREV" -D "$TMP/prev" -p "longrange-$PV-$plat.*" >/dev/null 2>&1 \
            && exe_of "$(ls "$TMP"/prev/longrange-"$PV"-"$plat".* | head -1)" "longrange-$PV-$plat" "$exe" "$TMP/old"; then
            out="dist/longrange-$V-$plat-from-$PV.zst"
            zstd -q -f -19 -T0 --patch-from="$TMP/old" "$TMP/new" -o "$out"
            echo "$out"
        else
            echo "exe_updates: no $plat archive in $PREV, no patch" >&2
        fi
    fi
done
