#!/usr/bin/env bash
# Fails if a release (draft or published) for v$VERSION already exists, or if the lookup fails.
# Drafts have no tag yet, so the tag check in check-release-version.sh cannot see them.
# Needs env VERSION, GITHUB_REPOSITORY and GH_TOKEN (write access, or drafts are invisible).
set -euo pipefail

tags="$(gh release list --repo "$GITHUB_REPOSITORY" --limit 1000 --json tagName --jq '.[].tagName')"
if grep -Fxq "v$VERSION" <<< "$tags"; then
  echo "::error::A release for v$VERSION already exists (possibly a draft). A release is never overwritten."
  exit 1
fi
