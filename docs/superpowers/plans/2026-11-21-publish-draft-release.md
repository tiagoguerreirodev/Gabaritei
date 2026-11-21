# Publish a Draft Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A manual dispatch of `release.yml` on `main` with a `version` input publishes a draft GitHub Release `v<version>` with the Windows `.exe` and macOS `.dmg` attached.

**Architecture:** Keep the existing `bundle` matrix. Add a version/tag guard script run early in each build leg, and a new `publish` job (`needs: bundle`) that downloads both artifacts and runs `gh release create --draft`. No third-party actions beyond `actions/download-artifact`.

**Tech Stack:** GitHub Actions, bash, `gh` CLI (preinstalled on runners), `actionlint`.

**Spec:** `docs/superpowers/specs/2026-11-21-publish-draft-release-design.md` (issue #146)

## Global Constraints

- Workflow-level `permissions` stays `contents: read`; only `publish` gets `contents: write`.
- Release is a **draft**, with `--generate-notes`, no `--prerelease`. Tag is `v<version>`, input has no `v` prefix.
- Publish only when `github.event_name == 'workflow_dispatch'`, `github.ref == 'refs/heads/main'`, and `inputs.version != ''`.
- `version` must equal `package.json` `version`, else fail before building. Existing tag `v<version>` fails the run; never overwrite.
- Never interpolate `inputs.version` into a script body; pass it through `env:`.
- PR runs stay build-only and cancellable; dispatch runs are never cancelled in progress.
- Fixed macOS line in the notes, in English: right-click > Open on first launch (app is ad-hoc signed, not notarized).
- Commits: Conventional Commits, English, imperative. Branch `issue-146-publish-draft-release`. PR body has `Closes #146`.

## Review Focus

- Version typed with a `v` prefix (`v0.2.0`): rejected with a clear message, not a silent `vv0.2.0` tag.
- Version given on a non-`main` ref: builds and uploads artifacts, `publish` is skipped, and the run says why.
- Tag already exists (even without a release): fails before the 60-minute builds.
- One matrix leg fails: `publish` does not run (default `needs` behavior; keep it, no `if: always()`).
- Hostile version string (`0.2.0; rm -rf /`, `$(...)`): rejected by the format check, never executed.

---

### Task 1: Version guard script

**Files:**
- Create: `.github/scripts/check-release-version.sh`
- Create: `.github/scripts/check-release-version.test.sh`

**Interfaces:**
- Produces: `check-release-version.sh` reads env `VERSION` (may be empty) and `PUBLISH` (`true`/`false`), run from the repo root. Exit 0 = ok or nothing to check; non-zero with a `::error::` line = reject. When `PUBLISH` is `true` it also fails if `git ls-remote --exit-code --tags origin refs/tags/v$VERSION` succeeds.

- [ ] **Step 1: Write `check-release-version.test.sh`.** A bash test that runs the script in a temp git repo (own `package.json` with version `1.2.3`, local bare repo as `origin`) and asserts exit codes for: empty `VERSION` → 0; `1.2.3` → 0; `1.2.4` (mismatch) → non-zero; `v1.2.3` → non-zero; `1.2.3; echo x` → non-zero and no `x` printed; `1.2.3-rc.1` against a package.json at `1.2.3-rc.1` → 0; tag `v1.2.3` present in origin with `PUBLISH=true` → non-zero, with `PUBLISH=false` → 0.
- [ ] **Step 2: Run it and see it fail** (script missing): `bash .github/scripts/check-release-version.test.sh` → fails.
- [ ] **Step 3: Implement the script.** Order: empty `VERSION` → exit 0; match `^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$` else error (mention "without a v prefix"); compare to `node -p "require('./package.json').version"`; tag check only when `PUBLISH=true`. `set -euo pipefail`; quote everything.
- [ ] **Step 4: Run the test; expect all cases pass.**
- [ ] **Step 5: Commit** `feat: add release version guard script`.

### Task 2: Workflow changes

**Files:**
- Modify: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: Task 1 script and its `VERSION` / `PUBLISH` env contract.

- [ ] **Step 1: Trigger and concurrency.** Add `workflow_dispatch.inputs.version` (string, not required, description says "empty = build only; must equal package.json version"). Set `cancel-in-progress: ${{ github.event_name == 'pull_request' }}`.
- [ ] **Step 2: Guard step.** In `bundle`, right after checkout, add a step running the script with `VERSION: ${{ inputs.version }}` and `PUBLISH: ${{ github.ref == 'refs/heads/main' }}` through `env:`. When `inputs.version` is non-empty and the ref is not `main`, print a `::warning::` that no release will be published.
- [ ] **Step 3: `publish` job.** `needs: bundle`, `runs-on: ubuntu-latest`, `timeout-minutes: 10`, job-level `permissions: contents: write`, `if:` per Global Constraints. Steps: `actions/download-artifact@v4` with `pattern: gabaritei-*`, `merge-multiple: true`, `path: bundles`; then `gh release create "v$VERSION" bundles/*.exe bundles/*.dmg --draft --generate-notes --notes "<macOS line>" --target "$GITHUB_SHA" --repo "$GITHUB_REPOSITORY"` with `GH_TOKEN: ${{ github.token }}` and `VERSION` via `env:`. Verified: with `--generate-notes`, `--notes` text is prepended. No checkout.
- [ ] **Step 4: Update the header comment** to say dispatch with a version on `main` publishes a draft release; still no notarization or auto-update.
- [ ] **Step 5: Lint.** Install `actionlint` (`brew install actionlint`) and run `actionlint .github/workflows/release.yml`; expect no output. Also run `npm run lint` (Prettier covers YAML).
- [ ] **Step 6: Commit** `feat: publish a draft GitHub Release from the release workflow`.

### Task 3: Prove it

**Files:** none.

- [ ] **Step 1:** Push the branch and open the PR (`Closes #146`; say how it was verified; no spec change). Confirm the PR run builds both legs and `publish` is skipped.
- [ ] **Step 2: Ask the owner** before any real publish. After merge, with their OK, dispatch on `main` with the current `package.json` version; confirm a draft release with both files and the macOS line; then delete the draft and its tag (ask first, this is outward-facing).
- [ ] **Step 3:** Report what was and wasn't verified. Do not claim the publish path works until Step 2 has run.
