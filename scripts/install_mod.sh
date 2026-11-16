#!/usr/bin/env bash
# Builds the STS2_MCP bridge mod (MIT, github.com/Gennadiyev/STS2MCP) from
# source at a pinned commit and installs it into the game's mods folder.
# The bot talks to the game through this mod.
#
#   scripts/install_mod.sh            # build + install
#   scripts/install_mod.sh --remove   # uninstall
#
# Needs: git, the .NET SDK (9 or newer), and Slay the Spire 2 installed.
set -euo pipefail

REPO=https://github.com/Gennadiyev/STS2MCP
COMMIT=55e064850a68f3b4cde7e5fd525bf9b2dec4e885   # "Fix game API compatibility with STS2 v0.107"

GAME_DIR="${STS2_GAME_DIR:-$HOME/.local/share/Steam/steamapps/common/Slay the Spire 2}"
MODS_DIR="$GAME_DIR/mods"
BUILD_DIR="$(cd "$(dirname "$0")/.." && pwd)/.build/STS2MCP"

if [[ ! -x "$GAME_DIR/SlayTheSpire2" ]]; then
    echo "Slay the Spire 2 not found at: $GAME_DIR" >&2
    echo "Set STS2_GAME_DIR to the game's install folder and retry." >&2
    exit 1
fi

if [[ "${1:-}" == "--remove" ]]; then
    rm -f "$MODS_DIR/STS2_MCP.dll" "$MODS_DIR/STS2_MCP.json" "$MODS_DIR/STS2_MCP.conf"
    echo "Removed STS2_MCP from $MODS_DIR"
    exit 0
fi

if [[ ! -d "$BUILD_DIR/.git" ]]; then
    mkdir -p "$(dirname "$BUILD_DIR")"
    git clone --quiet "$REPO" "$BUILD_DIR"
fi
git -C "$BUILD_DIR" fetch --quiet origin
git -C "$BUILD_DIR" checkout --quiet --force --detach "$COMMIT"

# Small additions the bot needs on top of the pinned commit (scripts/mod_patches).
for patch in "$(dirname "$0")"/mod_patches/*.patch; do
    [[ -e "$patch" ]] || continue
    git -C "$BUILD_DIR" apply "$(cd "$(dirname "$patch")" && pwd)/$(basename "$patch")"
done

dotnet build "$BUILD_DIR/STS2_MCP.csproj" -c Release -o "$BUILD_DIR/out" \
    -p:STS2GameDir="$GAME_DIR" --nologo -v quiet

mkdir -p "$MODS_DIR"
cp "$BUILD_DIR/out/STS2_MCP.dll" "$MODS_DIR/STS2_MCP.dll"
cp "$BUILD_DIR/mod_manifest.json" "$MODS_DIR/STS2_MCP.json"

cat <<EOF

Installed STS2_MCP into: $MODS_DIR

One manual step: start the game once, accept the mod consent dialog
(or enable mods under Settings -> Mods), then restart the game.
Check it works with:  curl -s http://localhost:15526/
EOF
