#!/usr/bin/env bash
# check_draws.sh: the draw-call gate of the art pass (ROADMAP "Art pass
# (pre-stream)", AP4): read the `frame_stats: draws` exit line that a run
# with --draw-stats printed (crates/game/src/draw_stats.rs) and fail unless
# its run maximum, and its fight-window maximum when there is one, is under
# the budget (500 draw calls a frame, docs/design/ui.md "Draw calls").
#
#   frame_stats: draws frames=75 mean=105.8 p95=109 max=109 indirect_max=97
#     direct_max=0 sorted_max=12 mode=culling fight_max=109
#
# The line must exist, count at least one frame and name a GPU
# preprocessing mode other than `none` (GL only, where the counter would
# miss the indirect draws; never one of our targets). With --fight, a
# `fight_max=` field is required too: `art_bench` and the `enemy_outpost`
# pair always print it, so its absence means the fight window never ran.
# CI's game-linux job runs this over every render smoke's log, and
# scripts/art_checks.sh over its `art_bench` run.
#
# Usage: scripts/check_draws.sh [--fight] <log> [budget]
#   budget defaults to 500; a frame passes with fewer draws than that.
set -euo pipefail

usage="usage: scripts/check_draws.sh [--fight] <log> [budget]"
need_fight=0
if [ "${1:-}" = "--fight" ]; then
  need_fight=1
  shift
fi
log="${1:?$usage}"
budget="${2:-500}"
case "$budget" in
  '' | *[!0-9]*) echo "error: budget must be a whole number, got '$budget'" >&2; exit 2 ;;
esac
if [ ! -f "$log" ]; then
  echo "error: no log at $log" >&2
  exit 2
fi

line=$(grep '^frame_stats: draws ' "$log" | tail -n 1 || true)
if [ -z "$line" ]; then
  echo "check_draws: FAIL no 'frame_stats: draws' line in $log (was --draw-stats passed?)"
  exit 1
fi
echo "$line"

# The value of field $1 (`name=value`), matched as a whole word so that
# `max` never reads `indirect_max`.
field() {
  local want=$1 word
  for word in $line; do
    if [ "${word%%=*}" = "$want" ]; then
      echo "${word#*=}"
      return
    fi
  done
}

# A whole number, or nothing when the field is missing or malformed.
number() {
  case "$1" in
    '' | *[!0-9]*) echo "" ;;
    *) echo "$1" ;;
  esac
}

fail=0
frames=$(number "$(field frames)")
max=$(number "$(field max)")
mode=$(field mode)
fight_raw=$(field fight_max)
fight=$(number "$fight_raw")
if [ -n "$fight_raw" ] && [ -z "$fight" ]; then
  echo "check_draws: FAIL fight_max=$fight_raw is not a number"
  fail=1
fi
if [ -z "$frames" ] || [ "$frames" -eq 0 ]; then
  echo "check_draws: FAIL no counted frame (frames=${frames:-missing})"
  fail=1
fi
if [ -z "$mode" ] || [ "$mode" = none ]; then
  echo "check_draws: FAIL mode=${mode:-missing}: the counter does not count that mode (GL only), so the gate fails rather than miscount"
  fail=1
fi
if [ -z "$max" ] || [ "$max" -ge "$budget" ]; then
  echo "check_draws: FAIL max=${max:-missing}, budget < $budget"
  fail=1
fi
if [ -n "$fight" ]; then
  if [ "$fight" -ge "$budget" ]; then
    echo "check_draws: FAIL fight_max=$fight, budget < $budget"
    fail=1
  fi
elif [ "$need_fight" -eq 1 ] && [ -z "$fight_raw" ]; then
  echo "check_draws: FAIL no fight_max field (the fight window never ran)"
  fail=1
fi
if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "check_draws: ok max=$max${fight:+ fight_max=$fight} < $budget mode=$mode ($log)"
