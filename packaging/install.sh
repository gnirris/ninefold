#!/bin/sh
# Put ninefold in the application menu, pointing at the binary in this checkout.
#
#   packaging/install.sh [host:port] [name]
#
# The menu entry starts the client at its title screen. The server given here, if any, becomes a
# second entry under it — right click in most menus — that connects straight away.
#
# Nothing is copied anywhere a package manager owns: the entry and the icon go under
# ~/.local/share, which is where a desktop looks for a user's own applications. Run it again
# after moving the checkout; a rebuild on its own needs nothing, since the path does not change.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
binary="$here/target/release/ninefold-client"
server="${1:-127.0.0.1:25565}"
player="${2:-}"

if [ ! -x "$binary" ]; then
    echo "no binary at $binary — run 'cargo build --release' first" >&2
    exit 1
fi

apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
mkdir -p "$apps" "$icons"

sed -e "s|@BINARY@|$binary|g" -e "s|@SERVER@|$server $player|g" \
    "$here/packaging/ninefold.desktop" > "$apps/ninefold.desktop"
chmod +x "$apps/ninefold.desktop"
cp "$here/packaging/ninefold.svg" "$icons/ninefold.svg"

# Both are best-effort: the entry works without them, it just may not appear until next login.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -f -t "${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor" || true

echo "ninefold is in the application menu, with $server under it"
