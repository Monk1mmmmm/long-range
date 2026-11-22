#!/usr/bin/env bash
# Removes what Long Range keeps outside its own folder: settings, logs, replays, your presets and
# downloaded maps (the user config folder). Then tells you to delete the game folder itself.
set -eu
case "$(uname -s)" in
    Darwin) DEFAULT="$HOME/Library/Application Support/LongRange" ;;
    *) DEFAULT="${XDG_CONFIG_HOME:-$HOME/.config}/longrange" ;;
esac
DIR="${LR_CONFIG:-$DEFAULT}"
GAME="$(cd "$(dirname "$0")" && pwd)"

if [ ! -d "$DIR" ]; then
    echo "No settings folder at $DIR - nothing to remove."
else
    echo "This deletes your Long Range settings, replays, presets and downloaded maps:"
    echo "  $DIR"
    printf "Delete it? [y/N] "
    read -r answer
    case "$answer" in
        y|Y|yes|YES) rm -rf -- "$DIR" && echo "Deleted $DIR" ;;
        *) echo "Kept $DIR" ;;
    esac
fi
echo
echo "The game itself is in this folder; delete it to finish uninstalling:"
echo "  $GAME"
echo "  (e.g.  cd .. && rm -rf \"$GAME\")"
