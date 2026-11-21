#!/usr/bin/env bash
# Guards a release dispatch. Run from the repo root with:
#   VERSION  the requested version without a "v" prefix; empty means build only
#   PUBLISH  "true" when this run will publish (so the tag must not exist yet)
set -euo pipefail

version="${VERSION:-}"
[ -z "$version" ] && exit 0

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "::error::Invalid version '$version': use X.Y.Z (optionally -suffix) without a v prefix."
  exit 1
fi

package_version="$(node -p "require('./package.json').version")"
if [ "$version" != "$package_version" ]; then
  echo "::error::Version '$version' does not match package.json ($package_version). Bump package.json first."
  exit 1
fi

if [ "${PUBLISH:-}" = "true" ] && git ls-remote --exit-code --tags origin "refs/tags/v$version" >/dev/null 2>&1; then
  echo "::error::Tag v$version already exists. A release is never overwritten."
  exit 1
fi
