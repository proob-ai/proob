#!/usr/bin/env bash
# Records docs/try.gif: the opening of a real /proob:try session, a time-skip card, and the
# session's final screen. The agent's minutes of work in between are not shown, because its
# tool calls print local paths. Needs vhs, ffmpeg, uv, and a signed-in claude; costs one
# /proob:try run (about $1).
set -euo pipefail
cd "$(dirname "$0")/../.."
demo=${PROOB_DEMO_DIR:-$HOME/proob-demo}
python3 -I - "$demo" <<'PY'
import json, os, sys
sys.path.insert(0, "bench/harness")
import blind
bug = next(b for b in json.load(open("bench/bugs.json")) if b["id"] == "semver-prerelease-token-order")
w = sys.argv[1]; src = blind.prepare(bug, w); os.rename(src, w + "/python-semver")
blind.venv(bug, w + "/python-semver", w)
PY
cargo build -q --locked -p proob
mkdir -p target/demo
vhs tools/demo/card.tape >/dev/null
vhs tools/demo/try.tape >/dev/null
fps=12
frames=$(ffprobe -v error -count_frames -show_entries stream=nb_read_frames -of csv=p=0 target/demo/raw.gif)
opening=$((frames - 17 * 20))  # drop the 16 s final screen (recorded at 20 fps); final.png replaces it
ffmpeg -y -v error -i target/demo/raw.gif -loop 1 -t 2.2 -i target/demo/card.png -loop 1 -t 13 -i target/demo/final.png \
  -filter_complex "[0]trim=end_frame=$opening,setpts=PTS-STARTPTS,fps=$fps,scale=1400:900,setsar=1,format=rgb24[a];\
[1]scale=1400:900,fps=$fps,setsar=1,format=rgb24[b];[2]scale=1400:900,fps=$fps,setsar=1,format=rgb24[c];\
[a][b][c]concat=n=3:v=1,split[x][y];[x]palettegen=stats_mode=diff[p];[y][p]paletteuse=dither=none" docs/try.gif
GIT_OPTIONAL_LOCKS=0 git -C "$demo/python-semver" status --porcelain | grep -q . && { echo "record: the demo repository changed" >&2; exit 1; }
echo "record: docs/try.gif ($(du -h docs/try.gif | cut -f1)); check target/demo/final.png before committing"
