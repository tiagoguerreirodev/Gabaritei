# Publish a draft GitHub Release from the release workflow (#146)

## Intent
The Release workflow builds the Windows and macOS installers but leaves them as 14-day workflow artifacts. The owner wants a GitHub Release with the bundles attached.

## Goals
- A manual dispatch on `main` with a `version` input creates a **draft** release `v<version>` with the NSIS `.exe` and the `.dmg` attached, with auto-generated notes plus a fixed macOS first-launch line.

## Non-goals
- Signing, notarization, auto-update (unchanged; spec §6).
- Tag-push triggers, committing from CI, version bumping by the workflow.

## Anti-goals
- Overwriting an existing tag or release.
- A build cancelled halfway by a second dispatch.
- Write permission outside the publish job.

## Constraints
- Build jobs stay as they are; PR runs remain build-only.
- No third-party release action; use the runner's `gh` CLI.

## Approaches
1. **`publish` job with `gh release create` (chosen).** No new dependency, build steps untouched.
2. Marketplace release action: adds a supply-chain dependency for one command.
3. `tauri-action`: would replace working build steps.

## Decisions (owner)
- Trigger: `workflow_dispatch` with a `version` input.
- Draft release, auto-generated notes.
- Version must equal `package.json`, else fail.
- Only `main` may publish; other refs stay build-only.
- Add a macOS "right-click > Open" line to the notes (English).

## Decisions (agent calls, approved with the design)
- `version` has no `v` prefix; the tag is `v<version>`.
- Empty `version` on dispatch means build-only, as today.
- Fail if `v<version>` already exists; never overwrite.
- No pre-release flag.
- Dispatch runs are not cancelled in progress (PR runs still are).
- `publish` has `needs: bundle`, its own `contents: write`, workflow default stays `contents: read`.

## Left to the builder
Exact step layout, artifact download paths, and the `gh` flags that attach the files and pin `--target` to the run's SHA.

## Verification
`actionlint`; the PR run proves the build still works. The publish path is checked by one dispatch on `main` after merge, then deleting the draft.
