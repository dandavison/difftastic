#!/usr/bin/env bash
# Repro: --display unified should produce standard unified diff output.
#
# Expected: difft accepts --display=unified and outputs --- a/, +++ b/, @@ headers.
# Bug: --display=unified is not recognized; difft errors on the unknown display mode.
#
# Run from repo root:
#   cargo build && bash sample_files/cli_tests/repro_unified_display.sh
set -euo pipefail

DIFFT="${1:-target/debug/difft}"

output=$("$DIFFT" --display=unified \
    sample_files/simple_1.js sample_files/simple_2.js 2>&1) || {
    echo "FAIL: difft exited with error for --display=unified"
    echo "$output"
    exit 1
}

for pattern in '--- a/' '+++ b/' '@@ -'; do
    if ! echo "$output" | grep -qF -- "$pattern"; then
        echo "FAIL: expected '$pattern' in unified output"
        echo "$output"
        exit 1
    fi
done

echo "PASS: --display=unified produces valid unified diff headers"
