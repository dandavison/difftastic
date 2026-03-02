#!/usr/bin/env bash
# Repro: --display unified should include intermediate context in merged hunks.
#
# When two changes are close enough to merge into a single hunk, the context
# lines between them must appear in the unified output. Here, `pass` sits
# between the foo() and bar() changes and must be present as a context line.
#
# Expected: unified output contains " pass" as a context line.
# Bug: intermediate context lines are missing; output jumps from one change
#      directly to the next.
#
# Run from repo root:
#   cargo build && bash sample_files/cli_tests/repro_merged_hunk_context.sh
set -euo pipefail

DIFFT="${1:-target/debug/difft}"

output=$("$DIFFT" --display=unified \
    sample_files/cli_tests/unified_lhs.py \
    sample_files/cli_tests/unified_rhs.py 2>&1)

if ! echo "$output" | grep -qF -- ' pass'; then
    echo "FAIL: intermediate context line 'pass' missing from merged hunk"
    echo "$output"
    exit 1
fi

echo "PASS: merged hunk includes intermediate context"
