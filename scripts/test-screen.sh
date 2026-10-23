#!/usr/bin/env bash
# Screenshots of the app at small window sizes, on a throwaway X screen.
#
#   scripts/test-screen.sh <list> [out-dir]
#
# Each line of <list> is "WxH name [ENV=value ...]", for example
#   360x780 phone-home SQD_SHOT_AT=3
#   480x800 pi-dupes SQD_SCREEN=dupes SQD_DUPES_PICK=1 SQD_SHOT_AT=3
#
# Starts a Xephyr screen in the middle of the primary monitor, runs the
# release build once per line with made-up data (MARKETING_DEMO), a separate
# HOME and English text, saves <out-dir>/<name>.png and always closes the
# screen again, even on Ctrl-C. Needs Xephyr, xdpyinfo, xrandr and
# python3-pil. Set TEST_SCREEN=WxH for another screen size (default 820x800),
# TEST_SCREEN_DEMO= (empty) for real data, e.g. a scan of a local test server.
set -euo pipefail
list=$1
out=${2:-target/screens}
repo=$(cd "$(dirname "$0")/.." && pwd)
bin=$repo/target/release/squirreldisk
[ -x "$bin" ] || { echo "build first: cargo build --release" >&2; exit 1; }
mkdir -p "$out"
out=$(cd "$out" && pwd)
home=$(mktemp -d)
demo=${TEST_SCREEN_DEMO-1}
size=${TEST_SCREEN:-820x800}
sw=${size%x*} sh=${size#*x}
# primary monitor geometry, e.g. "2560x1440+1920+0"
geo=$(xrandr --query | awk '/ connected primary/ {for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+x[0-9]+\+/) print $i}')
mw=${geo%%x*} rest=${geo#*x}; mh=${rest%%+*} rest=${rest#*+}; mx=${rest%%+*} my=${rest#*+}
x=$((mx + (mw - sw) / 2)) y=$((my + (mh - sh) / 2))
display=:${TEST_DISPLAY:-5}
Xephyr "$display" -screen "${size}+${x}+${y}" -title "SquirrelDisk test screen (closes itself)" -br -ac -nolisten tcp >/dev/null 2>&1 &
xp=$!
trap 'kill $xp 2>/dev/null; wait $xp 2>/dev/null; rm -rf "$home"' EXIT
for _ in $(seq 50); do xdpyinfo -display "$display" >/dev/null 2>&1 && break; sleep 0.1; done
while read -r win name extra; do
    [ -z "$win" ] || [ "${win:0:1}" = "#" ] && continue
    rm -f "$out/$name"-*.bmp
    # shellcheck disable=SC2086
    env -i PATH="$PATH" HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$home/.local/share" DISPLAY="$display" \
        SQUIRRELDISK_LANG=en ${demo:+MARKETING_DEMO=1} SQD_WINDOW="$win" SQD_SHOT="$out/$name" SQD_QUIT=1 $extra \
        timeout 40 "$bin" >/dev/null 2>"$out/$name.log" || true
    if python3 -c "import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2])" "$out/$name-0.bmp" "$out/$name.png" 2>/dev/null; then
        rm -f "$out/$name"-*.bmp
        echo "ok     $name"
    else
        echo "FAILED $name"
    fi
done < "$list"
