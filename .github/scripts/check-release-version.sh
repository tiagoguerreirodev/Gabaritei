#!/usr/bin/env bash
# Resolves and guards the release version. Run from the repo root with:
#   VERSION  the requested version without a "v" prefix; empty means "use package.json"
#   PUBLISH  "true" when this run will publish (so the tag must not exist yet)
# Writes version=<resolved> to $GITHUB_OUTPUT when that is set.
set -euo pipefail

package_version="$(node -p "require('./package.json').version")"
version="${VERSION:-$package_version}"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]]; then
  echo "::error::Invalid version '$version': use X.Y.Z (optionally -suffix) without a v prefix."
  exit 1
fi

if [ "$version" != "$package_version" ]; then
  echo "::error::Version '$version' does not match package.json ($package_version). Bump package.json first."
  exit 1
fi

if [ "${PUBLISH:-}" = "true" ] && git ls-remote --exit-code --tags origin "refs/tags/v$version" >/dev/null 2>&1; then
  echo "::error::Tag v$version already exists. A release is never overwritten."
  exit 1
fi

[ -n "${GITHUB_OUTPUT:-}" ] && echo "version=$version" >> "$GITHUB_OUTPUT"
exit 0
