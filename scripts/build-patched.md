# build-patched.sh — fork-image refresh workflow

A one-command refresh for the patched `makvitaly/appflowy_cloud` Docker image.
Pulls a chosen git ref from the upstream remote, applies the fork's small
patches on top, and produces a tagged image — without disturbing the
`feat/relation-in-row-detail-api` branch checkout.

## TL;DR

```bash
cd ~/dev/AppFlowy-Cloud
./scripts/build-patched.sh                    # default: upstream/main
./scripts/build-patched.sh --base v0.15.17    # build off a published tag
./scripts/build-patched.sh --base release-0.16  # build off an upstream branch
./scripts/build-patched.sh --base abc1234     # build off a commit SHA
```

Result: two image tags pointing at the same digest.

```
$ docker images | grep makvitaly/appflowy_cloud
makvitaly/appflowy_cloud   0.9.64-relations    abc123def...   231MB
makvitaly/appflowy_cloud   latest-relations    abc123def...   231MB
```

Then bump `~/server/stacks/appflowy/docker-compose.yml` (or its `.env`) to
the new tag and `docker compose up -d --force-recreate appflowy_cloud`.

## Choosing the base ref

By default the script builds against `upstream/main`. Override with
`--base <ref>`:

| Invocation | Tries to resolve as | Image tag |
|---|---|---|
| (no flag) | `upstream/main` (branch) | `:<nearest-semver>-relations` (e.g. `:0.9.64-relations`) |
| `--base v0.15.17` | `refs/tags/v0.15.17` (tag) | `:0.15.17-relations` (leading `v` stripped) |
| `--base release-0.16` | `upstream/release-0.16` (branch) | `:release-0.16-relations` |
| `--base abc1234` | `abc1234` (commit SHA) | `:abc1234-relations` |

Resolution order for an arbitrary `<ref>`:

1. `upstream/<ref>` — treat as a branch on the upstream remote
2. `refs/tags/<ref>` — treat as a tag
3. `<ref>` — treat as a commit SHA or other local ref

If none of those resolve, the script exits non-zero and prints what it tried.

If AppFlowy ever publishes a release branch or tag that matches the binary
shipped as `appflowyinc/appflowy_cloud:<version>` on Docker Hub, try `--base
<that-ref>`. If the patch doesn't apply cleanly (upstream refactored the
relation-handler code in that ref), the script tells you what to fix
manually — see "When a patch fails to apply" below.

> **Why this flag matters.** As of the last verification, the public
> `upstream/main` HEAD lags significantly behind what AppFlowy publishes as
> the official Docker image. Building from current `upstream/main` produces
> an older binary than e.g. `appflowyinc/appflowy_cloud:0.15.x`. The
> production self-hosted setup (operator side) compensates by routing the
> relation endpoint to our patched fork and everything else to the upstream
> image. `--base` lets us re-attempt the merge as soon as public source
> catches up.

## What it does, step by step

1. **`git fetch upstream --tags`** — pull latest branches and tags from the
   upstream remote.
2. **Resolve `--base`** — try `upstream/<ref>` → `refs/tags/<ref>` → `<ref>`
   directly, until one rev-parses cleanly. The matched kind (branch / tag /
   commit) is recorded for logging.
3. **Decide image tag** — for default `--base main`, use the nearest semver
   tag of the resolved commit (`git describe --tags --abbrev=0`). For an
   explicit `--base`, use the ref name as-is, sanitized to docker-tag-legal
   characters (`[a-zA-Z0-9._-]`, others → `-`); a leading `v` is stripped.
4. **Create a disposable worktree** at `/tmp/appflowy-build-patched`,
   detached at the resolved commit. The main checkout at
   `~/dev/AppFlowy-Cloud/` is untouched — it stays on
   `feat/relation-in-row-detail-api`.
5. **Apply every `patches/*.patch`** in lexicographic order via `git apply`.
   If any patch fails `--check`, the script exits with code 2 and prints
   recovery instructions (see below). No partial state — failed patches
   don't leave the worktree half-applied because `--check` runs first.
6. **`docker build`** the worktree (now resolved-base + patches), producing
   two tags:
   - `makvitaly/appflowy_cloud:<base-tag>-relations`
   - `makvitaly/appflowy_cloud:latest-relations`
   Cold build is 15–20 min. Subsequent runs with unchanged inputs are
   single-digit seconds (BuildKit cache hits every layer).
7. **Remove the worktree** (also runs on any error via `trap`).
8. **Print summary**: base ref, commit, patches applied, image tags, ID,
   size.

The Docker build target is the existing production `Dockerfile` (multi-stage,
final `runtime` image is a debian-slim with the compiled binary). It does NOT
run `cargo test` or `cargo check --tests` — those need rocksdb compiled,
which OOMs a 4 GiB Colima VM. The release-bin build alone does not pull in
rocksdb (it's a dev-only dep through `client-api`).

## Usage

### Just refresh against current upstream/main

```bash
./scripts/build-patched.sh
```

### Build off a specific upstream ref

```bash
./scripts/build-patched.sh --base v0.15.17        # tag
./scripts/build-patched.sh --base release-0.16    # branch on upstream
./scripts/build-patched.sh --base 9ad9ce58        # commit SHA
```

### Override the image name / worktree location

```bash
APPFLOWY_BUILD_IMAGE_NAME=ghcr.io/makvitaly/appflowy_cloud \
APPFLOWY_BUILD_WORKTREE=/var/tmp/appflowy-build \
  ./scripts/build-patched.sh --base v0.15.17
```

### Inspect what would be done without running docker

Patches and worktree creation are cheap; comment out the `docker build` line
locally and dry-run. (The script does not yet have a `--dry-run` flag — add
one if it becomes useful.)

## When a patch fails to apply

This happens when upstream refactored a file the patch touches, or when you
point `--base` at an old enough ref that the relation-handler code didn't
exist yet. Output looks like:

```
==> Applying 001-expose-relation-field-in-row-detail.patch
!! Patch 001-expose-relation-field-in-row-detail.patch does not apply cleanly to refs/tags/v0.7.2 (abc12345):
!!
    error: patch failed: src/api/workspace.rs:2739
    error: src/api/workspace.rs: patch does not apply
```

**The script does NOT auto-rebase.** Rebasing across a refactor is the kind
of merge that wants a human to look at it. The script just fails loudly so
you notice.

Recovery flow (manual, on the main checkout — not the worktree). The
example below uses `upstream/main`; if you were running with `--base
v0.15.17`, substitute that ref everywhere `upstream/main` appears.

```bash
cd ~/dev/AppFlowy-Cloud

# Bring upstream up to date and switch to the branch the patches come from.
git fetch upstream --tags
git checkout feat/relation-in-row-detail-api

# Replay the fork commits onto the base you want to support. Conflicts
# surface in your editor; resolve and 'git rebase --continue'.
git rebase upstream/main        # or: git rebase v0.15.17

# Re-export the patches from the now-fresh branch.
git format-patch upstream/main..feat/relation-in-row-detail-api -o patches/

# Rename to the convention NNN-<short-slug>.patch (overwrite the old files
# with the same names so the alphabetic order stays stable).
mv patches/0001-*.patch patches/001-expose-relation-field-in-row-detail.patch
mv patches/0002-*.patch patches/002-dockerfile-disable-lto.patch

# Sanity-check both apply cleanly now.
git worktree add --detach /tmp/check upstream/main
git -C /tmp/check apply --check patches/*.patch && echo OK
git worktree remove --force /tmp/check

# Push the rebased branch (force is expected after a rebase).
git push --force-with-lease origin feat/relation-in-row-detail-api

# Re-run the build script with the same --base you started with.
./scripts/build-patched.sh                 # or --base v0.15.17, etc.
```

## Adding a new patch

Same export flow as above, but use the next sequential prefix:

```bash
# Make a new commit on feat/relation-in-row-detail-api.
git checkout feat/relation-in-row-detail-api
$EDITOR …
git commit -m "feat: …"

# Export only the new commit. -1 limits to the last commit.
git format-patch -1 HEAD -o patches/

# Rename with the next number. Existing patches are 001-, 002-; this is 003-.
mv patches/0001-*.patch patches/003-<short-slug>.patch
```

The build script picks up `patches/*.patch` in alphabetic order, so a new
`003-foo.patch` is automatically applied after `002-dockerfile-disable-lto.patch`.

Removing a patch: delete the file from `patches/`. It will not be applied on
the next run.

## Caveats

- **Build memory floor: ~6 GiB Colima.** Even with `lto=off` +
  `opt-level=2` set in the Dockerfile, the rustc compile of
  `appflowy-cloud`'s lib crate (sqlx + Actix macros expand into massive
  HIR) peaks at ~5 GiB. Empirically validated on **6 GiB Colima** (4:22
  cold, 2.9 s idempotent). At 4 GiB the lib compile OOMs. If your Docker
  VM is sized for runtime (4 GiB), bump it temporarily for the build:

  ```bash
  colima stop
  colima start --memory 6
  ./scripts/build-patched.sh
  # optional: drop back to 4 GiB once the image is built
  colima stop
  colima start --memory 4
  ```

  Runtime itself is fine on 4 GiB — only the build is memory-hungry.
- The script assumes `upstream` remote exists and points at
  `AppFlowy-IO/AppFlowy-Cloud`. Verify with `git remote -v` if a run fails on
  the first `git fetch`.
- `docker build` uses the default BuildKit cache stored inside Colima/Docker
  Desktop. If you ever run `docker system prune --all` the next build is
  cold (15–20 min).
- The patched image is **not pushed anywhere**. The artifact is the local
  Docker image. The operator pulls it into `~/server/stacks/appflowy/` by
  bumping the `image:` line in `docker-compose.yml`.
- Tests are deliberately not run by this script. `cargo test` for this
  workspace transitively pulls `rocksdb` (a heavy C++ build) through
  `client-api`'s test-helper dependency, and the C++ compile OOMs a 4 GiB
  Colima VM. Trust the upstream PR / CI for test coverage, or run tests on a
  beefier machine separately.
