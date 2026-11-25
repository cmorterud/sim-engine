#!/usr/bin/env bash
# Takes `--landscape` pictures of each map with one build of the program, for the Landscape
# workflow:
#
#     scripts/landscape_maps.sh <program> <out dir> <map>...
#
# The software renderer on CI runners sometimes crashes on a view, on main's build as well as a
# branch's, so a map whose run fails is tried once more. If it fails again the other maps still
# go ahead, and the failure is reported as a warning, so that the pictures that were taken
# still reach the artifact. The script fails only if no map could be pictured at all.
set -u
program="$1"
out="$2"
shift 2
pictured=0
failed=()
for map in "$@"; do
  for attempt in 1 2; do
    xvfb-run -a -s "-screen 0 1440x900x24" "$program" --map "$map" --landscape "$out/$map"
    status=$?
    if [ "$status" -eq 0 ]; then
      pictured=$((pictured + 1))
      continue 2
    fi
    echo "::warning::landscape: $map failed on attempt $attempt (exit $status)"
  done
  failed+=("$map")
done
if [ "${#failed[@]}" -gt 0 ]; then
  echo "::warning::landscape: no complete pictures of ${failed[*]} in $out"
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    echo "Landscape: the renderer crashed twice on ${failed[*]} for $out; those maps' pictures may be partial or missing." >> "$GITHUB_STEP_SUMMARY"
  fi
fi
[ "$pictured" -gt 0 ]
