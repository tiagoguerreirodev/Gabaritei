#!/usr/bin/env bash
# Tests for check-release-version.sh. Run: bash .github/scripts/check-release-version.test.sh
set -uo pipefail

SCRIPT="$(cd "$(dirname "$0")" && pwd)/check-release-version.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

git init -q --bare "$TMP/origin.git"
git init -q "$TMP/repo"
cd "$TMP/repo"
git remote add origin "$TMP/origin.git"
printf '{"name":"x","version":"1.2.3"}\n' > package.json

failures=0
# run <label> <expected: ok|fail> <VERSION> <PUBLISH>
run() {
  local label=$1 expected=$2 version=$3 publish=$4 out status
  out=$(VERSION="$version" PUBLISH="$publish" bash "$SCRIPT" 2>&1)
  status=$?
  if { [ "$expected" = ok ] && [ $status -eq 0 ]; } || { [ "$expected" = fail ] && [ $status -eq 1 ]; }; then
    echo "PASS: $label"
  else
    echo "FAIL: $label (exit $status, expected $expected)"
    echo "$out" | sed 's/^/    /'
    failures=$((failures + 1))
  fi
  LAST_OUT=$out
}

run "empty version is build-only" ok "" true
run "matching version" ok "1.2.3" true
run "mismatching version" fail "1.2.4" true
run "v prefix rejected" fail "v1.2.3" true
run "v prefix error explains" fail "v1.2.3" true
case "$LAST_OUT" in *"without a v prefix"*) echo "PASS: v prefix message" ;; *) echo "FAIL: v prefix message"; failures=$((failures + 1)) ;; esac

run "hostile version rejected" fail '1.2.3; echo x' true
if printf '%s\n' "$LAST_OUT" | grep -qx 'x'; then echo "FAIL: hostile version was executed"; failures=$((failures + 1)); else echo "PASS: hostile version not executed"; fi

printf '{"name":"x","version":"1.2.3-rc.1"}\n' > package.json
run "prerelease version matches" ok "1.2.3-rc.1" true
printf '{"name":"x","version":"1.2.3"}\n' > package.json

git -c user.name=t -c user.email=t@t commit -q --allow-empty -m init
git tag v1.2.3
git push -q origin v1.2.3
run "existing tag fails when publishing" fail "1.2.3" true
run "existing tag ignored when not publishing" ok "1.2.3" false

[ $failures -eq 0 ] && echo "all passed" || { echo "$failures failed"; exit 1; }
