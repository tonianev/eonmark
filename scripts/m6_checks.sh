#!/usr/bin/env bash
# m6_checks.sh: the windowed proxies of ROADMAP M6 slice 3 (line 5, and
# line 6's automated proxy: the game-over statistics screenshot standing in
# for ci_testing, M6 owner decision 9). `just m6-checks` builds both
# binaries and runs it; a local, PR-pasted check like m5b_checks.sh. Every
# run is 1280 x 720 (the stream's logical size, owner decision 10).
#
#   1. Line 5: `sim-cli verify` of the scripted_match golden, then
#      `--scenario scripted_match` at x1, and at x4 with `--pause-at-tick
#      600 --pause-seconds 3` (whose screenshot at tick 600 is the paused
#      frame, m6-paused.png). Both must print `outcome: victory player=1
#      decisive=true reason=surrender tick=T hash=H` with H the golden's,
#      the x4 run `pause: on tick=600` and `pause: off tick=600 after_s=`,
#      and both replays must `sim-cli verify` to `OK final_hash=H ticks=T`.
#   2. The New Game selector: a plain launch with `--difficulty hard
#      --personality rush --seed 42`, first for 8 s with --screenshot (the
#      menu, m6-new-game.png), then with `--new-game-after-seconds 3`: it
#      must print `match: new game vs hard personality=rush seed=42`, and
#      `sim-cli match-log` of its replay must name Hard and rush at seed 42
#      and verify OK.
#   3. The game-over screen from the human seat: T is read from `play-bots
#      --a passive --b standard --personality-b rush --seeds 3..3` (a human
#      who gives no order is Passive); the game runs `--skip-menu
#      --difficulty standard --personality rush --seed 3 --fast-forward-to
#      <T-400> --screenshot-at <T>:m6-game-over.png`. Its `outcome:` tick
#      must be T, it must print the `match_log:` line, and its replay
#      (recorded from T-400, with no human order before it) must verify to
#      the outcome's hash.
#   4. Play Again: the same run's `--play-again-after-seconds 2` presses
#      Play Again on that screen: a second `match:` line, a second replay,
#      and both replays verify OK.
#   5. No eonmark (and no Xvfb) process is left.
#
# On Linux without a display every game runs under `xvfb-run` on lavapipe,
# where x1 runs about 10-13 ticks a second and "x4" about 18-21 (the 8-tick
# cap per frame binds at 2-3 fps), so the deadlines carry headroom and the
# gate is the `outcome:` line; x4 throughput is shown by the headless rate
# test (`speed::tests`) and on the owner's Mac (docs/PLAYTEST.md).
#
# Usage: scripts/m6_checks.sh <eonmark binary> <sim-cli binary> [out-dir]
# Env:   M6_SECONDS=360    the x1 scripted_match run's deadline
#        M6_X4_SECONDS=240 the x4 run's deadline
#        M6_GO_SECONDS=90  the game-over and Play Again run's deadline
#        ONLY="1 3"        run only these steps (5 always runs)
set -euo pipefail

usage="usage: scripts/m6_checks.sh <eonmark binary> <sim-cli binary> [out-dir]"
bin="${1:?$usage}"
simcli="${2:?$usage}"
out="${3:-}"
if [ -z "$out" ]; then
  out="$(mktemp -d "${TMPDIR:-/tmp}/eonmark-m6-checks.XXXXXX")"
fi
mkdir -p "$out"
out="$(cd "$out" && pwd)"
bin="$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")"
simcli="$(cd "$(dirname "$simcli")" && pwd)/$(basename "$simcli")"
x1_seconds="${M6_SECONDS:-360}"
x4_seconds="${M6_X4_SECONDS:-240}"
go_seconds="${M6_GO_SECONDS:-90}"
only="${ONLY:-1 2 3 4}"

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# Background mode for every windowed run (rule 25).
export EONMARK_BACKGROUND=1

fail=0
bad() { echo "FAIL  $*"; fail=1; }
ok() { echo "PASS  $*"; }
want_step() { case " $only " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# Linux without a display: a virtual one on lavapipe.
wrap=()
if [ "$(uname)" = Linux ] && [ -z "${DISPLAY:-}" ]; then
  icd=$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -1 || true)
  if [ -z "$icd" ] || ! command -v xvfb-run >/dev/null; then
    echo "error: no display, and xvfb-run or lavapipe is missing (apt-get install xvfb mesa-vulkan-drivers)" >&2
    exit 1
  fi
  wrap=(xvfb-run -a -s "-screen 0 1280x800x24" env VK_ICD_FILENAMES="$icd")
fi

# Run the game with "$@" into the log $1; sets `status`.
status=0
game() {
  local log=$1
  shift
  echo "+ ${wrap[*]:-} $bin $* > $log"
  status=0
  ${wrap[@]+"${wrap[@]}"} "$bin" --window-size 1280x720 "$@" >"$log" 2>&1 || status=$?
}

no_errors() {
  local errors
  errors=$(sed 's/\x1b\[[0-9;]*m//g' "$1" | grep -E '^[^ ]+ +ERROR ' || true)
  if [ -z "$errors" ]; then ok "no ERROR line in $1"; else bad "ERROR lines in $1:"; echo "$errors"; fi
}

# `sim-cli verify` of $1 must print exactly $2.
verify_is() {
  local got
  got="$("$simcli" verify "$1" 2>&1 || true)"
  echo "    $got"
  if [ "$got" = "$2" ]; then ok "$(basename "$1") verifies: $got"; else bad "$(basename "$1"): expected '$2'"; fi
}

if want_step 1; then
  echo "==> 1. line 5: scripted_match at x1, and at x4 with a pause at tick 600"
  fixture=crates/sim/tests/fixtures/scripted_match.eonreplay
  golden="$(tr -d '[:space:]' < crates/sim/tests/fixtures/scripted_match.hash)"
  line="$("$simcli" verify "$fixture" 2>&1)"
  echo "    $line"
  case "$line" in
    "OK final_hash=$golden ticks="*) ok "the golden verifies to scripted_match.hash $golden" ;;
    *) bad "the golden does not verify to $golden"; exit 1 ;;
  esac
  ticks="${line##* ticks=}"
  want_outcome="outcome: victory player=1 decisive=true reason=surrender tick=$ticks hash=$golden"
  for run in x1 x4; do
    dir="$out/$run-replays"
    rm -rf "$dir"; mkdir -p "$dir"
    log="$out/$run.log"
    if [ "$run" = x1 ]; then
      game "$log" --scenario scripted_match --speed 1 --replay-dir "$dir" --exit-after-seconds "$x1_seconds"
    else
      rm -f "$out/m6-paused.png"
      game "$log" --scenario scripted_match --speed 4 --pause-at-tick 600 --pause-seconds 3 \
        --screenshot-at "600:$out/m6-paused.png" --replay-dir "$dir" --exit-after-seconds "$x4_seconds"
    fi
    if [ "$status" -eq 0 ]; then ok "$run: the game exited 0"; else bad "$run: the game exited $status"; fi
    no_errors "$log"
    grep -E '^(speed|pause|outcome|driver): ' "$log" | sed 's/^/    /' || true
    if grep -qx "$want_outcome" "$log"; then ok "$run: $want_outcome"; else bad "$run: no '$want_outcome'"; fi
    replay="$(sed -n 's/^replay: //p' "$log" | head -1)"
    if [ -n "$replay" ] && [ -f "$replay" ]; then
      verify_is "$replay" "OK final_hash=$golden ticks=$ticks"
    else
      bad "$run: no replay file"
    fi
  done
  if grep -qx 'pause: on tick=600' "$out/x4.log"; then ok "x4: the pause landed on tick 600"; else bad "x4: no 'pause: on tick=600'"; fi
  if grep -q '^pause: off tick=600 after_s=' "$out/x4.log"; then ok "x4: $(grep '^pause: off' "$out/x4.log")"; else bad "x4: no 'pause: off tick=600 after_s=...'"; fi
  if [ -s "$out/m6-paused.png" ]; then ok "paused screenshot $out/m6-paused.png"; else bad "no paused screenshot"; fi
fi

if want_step 2; then
  echo
  echo "==> 2. the New Game selector: Hard, rush, seed 42"
  dir="$out/menu-replays"
  rm -rf "$dir"; mkdir -p "$dir"
  rm -f "$out/m6-new-game.png"
  game "$out/menu.log" --difficulty hard --personality rush --seed 42 --replay-dir "$dir" \
    --screenshot "$out/m6-new-game.png" --exit-after-seconds 8
  if [ "$status" -eq 0 ]; then ok "the menu run exited 0"; else bad "the menu run exited $status"; fi
  no_errors "$out/menu.log"
  if [ -s "$out/m6-new-game.png" ]; then ok "menu screenshot $out/m6-new-game.png"; else bad "no menu screenshot"; fi
  if [ -n "$(ls -A "$dir")" ]; then bad "the menu run recorded $(ls "$dir")"; else ok "nothing recorded while the menu was open"; fi
  dir="$out/new-game-replays"
  rm -rf "$dir"; mkdir -p "$dir"
  game "$out/new-game.log" --difficulty hard --personality rush --seed 42 --replay-dir "$dir" \
    --new-game-after-seconds 3 --exit-after-seconds 45
  if [ "$status" -eq 0 ]; then ok "the New Game run exited 0"; else bad "the New Game run exited $status"; fi
  no_errors "$out/new-game.log"
  want="match: new game vs hard personality=rush seed=42 map=plains_1v1"
  if grep -qx "$want" "$out/new-game.log"; then ok "$want"; else bad "no '$want'"; fi
  replay="$(sed -n 's/^replay: //p' "$out/new-game.log" | head -1)"
  if [ -n "$replay" ] && [ -f "$replay" ]; then
    row="$("$simcli" match-log "$replay" 2>&1 | head -1 || true)"
    echo "    $row"
    case "$row" in
      *"| 42 | Hard | rush | "*"| OK 0x"*) ok "the replay's header is Hard, rush, seed 42 and it verifies" ;;
      *) bad "match-log row: expected seed 42, Hard, rush and OK" ;;
    esac
  else
    bad "no New Game replay"
  fi
fi

if want_step 3 || want_step 4; then
  echo
  echo "==> 3 and 4. the game-over screen from the human seat, then Play Again"
  row="$("$simcli" play-bots --a passive --b standard --personality-b rush --seeds 3..3 --max-ticks 36000 2>/dev/null | sed -n 2p)"
  t="$(printf '%s\n' "$row" | cut -d, -f4)"
  echo "    play-bots seed 3: $(printf '%s\n' "$row" | cut -d, -f1-5)"
  from=$((t - 400))
  dir="$out/game-over-replays"
  rm -rf "$dir"; mkdir -p "$dir"
  rm -f "$out/m6-game-over.png"
  log="$out/game-over.log"
  game "$log" --skip-menu --difficulty standard --personality rush --seed 3 --fast-forward-to "$from" \
    --screenshot-at "$t:$out/m6-game-over.png" --play-again-after-seconds 2 --replay-dir "$dir" \
    --exit-after-seconds "$go_seconds"
  if [ "$status" -eq 0 ]; then ok "the game exited 0"; else bad "the game exited $status"; fi
  no_errors "$log"
  grep -E '^(fast_forward|match|outcome|match_log|play-again|replay): ' "$log" | sed 's/^/    /' || true
  outcome="$(grep '^outcome: ' "$log" | head -1 || true)"
  case "$outcome" in
    "outcome: victory player=1 decisive=true reason=seat_captured tick=$t hash="*) ok "3: the outcome is play-bots' tick $t" ;;
    *) bad "3: expected 'outcome: victory player=1 ... tick=$t'" ;;
  esac
  if grep -q '^match_log: | ' "$log"; then ok "3: the match_log: line"; else bad "3: no match_log: line"; fi
  if [ -s "$out/m6-game-over.png" ]; then ok "3: game-over screenshot $out/m6-game-over.png"; else bad "3: no game-over screenshot"; fi
  hash="${outcome##* hash=}"
  replays=()
  while IFS= read -r r; do replays+=("$r"); done < <(sed -n 's/^replay: //p' "$log")
  if [ "${#replays[@]}" -ge 1 ] && [ -f "${replays[0]}" ]; then
    verify_is "${replays[0]}" "OK final_hash=$hash ticks=$t"
  else
    bad "3: no replay"
  fi
  matches="$(grep -c '^match: ' "$log" || true)"
  if grep -q '^play-again: pressed after ' "$log" && [ "$matches" -eq 2 ]; then
    ok "4: Play Again started a second match: $(grep '^match: ' "$log" | tail -1)"
  else
    bad "4: expected 'play-again: pressed' and two match: lines (got $matches)"
  fi
  if [ "${#replays[@]}" -eq 2 ] && [ -f "${replays[1]}" ]; then
    got="$("$simcli" verify "${replays[1]}" 2>&1 || true)"
    echo "    $got"
    case "$got" in
      "OK final_hash="*) ok "4: the Play Again replay verifies" ;;
      *) bad "4: the Play Again replay: $got" ;;
    esac
  else
    bad "4: expected two replays, got ${#replays[@]}"
  fi
fi

echo
echo "==> 5. no game process left"
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if pgrep -x eonmark >/dev/null || pgrep -x Xvfb >/dev/null; then sleep 1; else break; fi
done
if pgrep -x eonmark >/dev/null; then bad "eonmark still running:"; pgrep -xl eonmark; else ok "pgrep -x eonmark is empty"; fi
if [ ${#wrap[@]} -gt 0 ]; then
  if pgrep -x Xvfb >/dev/null; then bad "Xvfb still running:"; pgrep -xl Xvfb; else ok "pgrep -x Xvfb is empty"; fi
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "m6_checks: FAIL ($out)"
  exit 1
fi
echo "m6_checks: ok ($out)"
