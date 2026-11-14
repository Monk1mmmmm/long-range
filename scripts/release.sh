#!/usr/bin/env bash
# Builds release bundles (game + map editor + assets) and publishes them as a GitHub release.
#
#   scripts/release.sh                    build Linux and Windows bundles, upload both
#   scripts/release.sh --only linux       build just one platform (linux | windows)
#   scripts/release.sh --no-upload        build only, leave the archives in dist/
#   scripts/release.sh --tag v0.3.0       release tag (default: v<version from Cargo.toml>)
#   scripts/release.sh --draft            create the GitHub release as a draft
#
# Requirements: Rust (rustup). For the Windows bundle on Linux: the cross target and MinGW
#   rustup target add x86_64-pc-windows-gnu      and the package mingw-w64-gcc (x86_64-w64-mingw32-gcc),
#   plus curl and unzip (the bundle ships Microsoft's dxcompiler.dll, downloaded once into target/dxc)
# For the upload: the GitHub CLI `gh`, logged in (`gh auth login`), and the commit you build pushed.
set -euo pipefail
cd "$(dirname "$0")/.."

ONLY="" UPLOAD=1 TAG="" DRAFT=""
while [ $# -gt 0 ]; do
    case "$1" in
        --only) ONLY="$2"; shift 2 ;;
        --no-upload) UPLOAD=0; shift ;;
        --tag) TAG="$2"; shift 2 ;;
        --draft) DRAFT="--draft"; shift ;;
        -h|--help) sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1 (try --help)" >&2; exit 2 ;;
    esac
done
case "$ONLY" in ""|linux|windows) ;; *) echo "--only takes 'linux' or 'windows'" >&2; exit 2 ;; esac

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TAG=${TAG:-v$VERSION}
mkdir -p dist
FILES=()

# The two programs are built one after the other: linking both at once needs more memory
# than an 8 GB machine has and ends in swap thrashing.
build_bins() {
    cargo build --release --bin longrange "$@"
    cargo build --release --bin lr-editor "$@"
}

# The MinGW build cannot link Microsoft's DXC shader compiler statically (wgpu supports that with MSVC
# only), so its bundle ships dxcompiler.dll. Without DXC, DirectX 12 falls back to FXC: black 3D view.
DXC_URL=https://github.com/microsoft/DirectXShaderCompiler/releases/download/v1.9.2609/dxc_2026_09_29.zip
DXC_DIR=""
fetch_dxc() {
    local d=target/dxc
    if [ -s "$d/dxcompiler.dll" ] && [ -s "$d/dxil.dll" ]; then return; fi
    echo "== downloading the DXC shader compiler"
    mkdir -p "$d"
    curl -fsSL -o "$d/dxc.zip" "$DXC_URL"
    unzip -p "$d/dxc.zip" 'bin?x64?dxcompiler.dll' > "$d/dxcompiler.dll"
    unzip -p "$d/dxc.zip" 'bin?x64?dxil.dll' > "$d/dxil.dll"
    unzip -p "$d/dxc.zip" LICENSE-MS.txt > "$d/LICENSE-DXC-MS.txt"
    unzip -p "$d/dxc.zip" LICENSE-LLVM.txt > "$d/LICENSE-DXC-LLVM.txt"
    unlink "$d/dxc.zip"
}

bundle() { # <platform name> <cargo output dir> <exe suffix>
    local name="longrange-$VERSION-$1" out
    local dir="dist/${name:?}"
    # Start from an empty bundle folder.
    if [ -d "$dir" ]; then
        find "$dir" -mindepth 1 -delete
    fi
    mkdir -p "$dir"
    cp "$2/longrange$3" "$2/lr-editor$3" "$dir/"
    if [ -n "$DXC_DIR" ]; then
        cp "$DXC_DIR/dxcompiler.dll" "$DXC_DIR/dxil.dll" "$DXC_DIR"/LICENSE-DXC-*.txt "$dir/"
    fi
    cp -r assets "$dir/assets"
    cp README.md CHANGELOG.md "$dir/"
    if [ -f LICENSE ]; then cp LICENSE "$dir/"; fi
    if [ "$3" = ".exe" ]; then
        out="dist/$name.zip"
        if [ -f "$out" ]; then unlink "$out"; fi
        (cd dist && zip -qr "$name.zip" "$name")
    else
        out="dist/$name.tar.gz"
        tar -C dist -czf "$out" "$name"
    fi
    FILES+=("$out")
    echo "bundled $out ($(du -h "$out" | cut -f1))"
}

if [ -z "$ONLY" ] || [ "$ONLY" = linux ]; then
    echo "== building Linux x86_64"
    build_bins
    bundle linux-x86_64 target/release ""
fi
if [ -z "$ONLY" ] || [ "$ONLY" = windows ]; then
    echo "== building Windows x86_64"
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*)
            build_bins
            bundle windows-x86_64 target/release .exe ;;
        *)
            if ! rustup target list --installed 2>/dev/null | grep -q x86_64-pc-windows-gnu || ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
                echo "Windows cross build needs: rustup target add x86_64-pc-windows-gnu, and mingw-w64 gcc" >&2
                exit 1
            fi
            build_bins --target x86_64-pc-windows-gnu
            fetch_dxc
            DXC_DIR=target/dxc
            bundle windows-x86_64 target/x86_64-pc-windows-gnu/release .exe ;;
    esac
fi

if [ "$UPLOAD" = 0 ]; then
    echo "skipping upload (--no-upload). Archives: ${FILES[*]}"
    exit 0
fi
command -v gh >/dev/null || { echo "GitHub CLI 'gh' not found; rerun with --no-upload or install it" >&2; exit 1; }
if [ -n "$(git status --porcelain)" ]; then
    echo "The working tree has uncommitted changes, so the release would not match any commit." >&2
    echo "Commit and push first, or rerun with --no-upload." >&2
    exit 1
fi
HEAD_SHA=$(git rev-parse HEAD)
git fetch -q origin
if ! git branch -r --contains "$HEAD_SHA" | grep -q .; then
    echo "Commit $HEAD_SHA is not on the remote yet. Push it first, or rerun with --no-upload." >&2
    exit 1
fi
if gh release view "$TAG" >/dev/null 2>&1; then
    echo "== release $TAG exists, replacing its files"
    gh release upload "$TAG" "${FILES[@]}" --clobber
else
    echo "== creating release $TAG"
    # Release text: the section for this tag in CHANGELOG.md, or GitHub's generated notes without one.
    NOTES=dist/release-notes.md
    awk -v tag="## $TAG" '$0 == tag {on=1; next} /^## / {on=0} on' CHANGELOG.md > "$NOTES"
    if [ -s "$NOTES" ]; then
        gh release create "$TAG" "${FILES[@]}" --target "$HEAD_SHA" --title "Long Range $TAG" --notes-file "$NOTES" $DRAFT
    else
        echo "no '## $TAG' section in CHANGELOG.md, using generated notes"
        gh release create "$TAG" "${FILES[@]}" --target "$HEAD_SHA" --title "Long Range $TAG" --generate-notes $DRAFT
    fi
fi
gh release view "$TAG" --json url --jq .url
