#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Riviera contributors
#
# Validates the sample models with IfcOpenShell's validator, EXPRESS rules included:
# the schema check alone misses where-rules and inverse cardinalities (a type with two
# IfcRelDefinesByType, an opening voiding nothing, a zero IfcPositiveRatioMeasure).
#
#   tools/validate-samples.sh [name ...]     # all samples, or the named ones
#
# Needs Python with ifcopenshell and pytest (the rule executor imports it). Prints one line a
# sample with issues and exits non-zero if any has them; full reports go to $OUT (a temporary
# folder unless set).
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
samples="$here/test-data/samples"
out="${OUT:-$(mktemp -d)}"
if [ $# -gt 0 ]; then
  names=("$@")
else
  names=()
  for f in "$samples"/*.ifc; do names+=("$(basename "$f" .ifc)"); done
fi
bad=0
for n in "${names[@]}"; do
  report="$out/$n.txt"
  # Run from the report folder, not the repository, so no local module shadows a library's.
  (cd "$out" && nice -n 19 python3 -m ifcopenshell.validate --rules "$samples/$n.ifc" >"$report" 2>&1) || true
  if ! grep -q "No validation issues" "$report"; then
    bad=1
    rules=$(grep -E '^Rule ' "$report" | sort -u | tr '\n' ' ')
    echo "$n: ${rules:-see $report}"
  fi
done
[ $bad = 0 ] && echo "${#names[@]} samples, no validation issues"
echo "reports: $out"
exit $bad
