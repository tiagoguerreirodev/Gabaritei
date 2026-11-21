#!/usr/bin/env bash
# Tests for check-release-absent.sh with a fake gh. Run: bash .github/scripts/check-release-absent.test.sh
set -uo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/check-release-absent.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir "$TMP/bin"
# Fake gh: prints $FAKE_GH_TAGS (one per line) or fails when FAKE_GH_FAIL=1.
cat > "$TMP/bin/gh" <<'GH'
#!/usr/bin/env bash
[ "${FAKE_GH_FAIL:-0}" = 1 ] && { echo "gh: boom" >&2; exit 4; }
printf '%s' "${FAKE_GH_TAGS:-}"
GH
chmod +x "$TMP/bin/gh"

failures=0
# run <label> <expected exit> <tags> <gh fails 0|1>
run() {
  local label=$1 expected=$2 tags=$3 fail=$4 out status
  out=$(PATH="$TMP/bin:$PATH" VERSION=1.2.3 GITHUB_REPOSITORY=o/r FAKE_GH_TAGS="$tags" FAKE_GH_FAIL="$fail" bash "$SCRIPT" 2>&1)
  status=$?
  if [ "$status" = "$expected" ]; then echo "PASS: $label"; else
    echo "FAIL: $label (exit $status, expected $expected)"; echo "$out" | sed 's/^/    /'; failures=$((failures + 1)); fi
}

run "no releases" 0 "" 0
run "other releases only" 0 $'v1.2.2\nv1.2.30\n' 0
run "existing release (draft or not)" 1 $'v1.2.2\nv1.2.3\n' 0
run "lookup failure is not read as absent (gh exit code propagates)" 4 "" 1

[ $failures -eq 0 ] && echo "all passed" || { echo "$failures failed"; exit 1; }
