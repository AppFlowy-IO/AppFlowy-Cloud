# build-patched.sh — fork-image refresh workflow

A one-command refresh for the patched `makvitaly/appflowy_cloud` Docker image.
Pulls latest `upstream/main`, applies the fork's small patches on top, and
produces a tagged image — without disturbing the `feat/relation-in-row-detail-api`
branch checkout.

## TL;DR

```bash
cd ~/dev/AppFlowy-Cloud
./scripts/build-patched.sh
```

Result: two image tags pointing at the same digest.

```
$ docker images | grep makvitaly/appflowy_cloud
makvitaly/appflowy_cloud   0.9.64-relations    abc123def...   233 MB
makvitaly/appflowy_cloud   latest-relations    abc123def...   233 MB
```

Then bump `~/server/stacks/appflowy/docker-compose.yml` (or its `.env`) to
the new tag and `docker compose up -d --force-recreate appflowy_cloud`.

## What it does, step by step

1. **`git fetch upstream main`** — pull latest upstream HEAD into the local
   `upstream/main` ref.
2. **Detect upstream version** — `git describe --tags --abbrev=0 upstream/main`.
   AppFlowy-Cloud tags releases as plain semver (`0.9.64`, `0.9.65`, ...). If
   `upstream/main` is ahead of every tag, falls back to `git-<short-sha>`.
3. **Create a disposable worktree** at `/tmp/appflowy-build-patched`, detached
   at the `upstream/main` SHA. The `~/dev/AppFlowy-Cloud/` main checkout is
   untouched — it can stay on `feat/relation-in-row-detail-api` for ongoing
   work.
4. **Apply every `patches/*.patch`** in lexicographic order via `git apply`.
   If any patch fails the `--check`, the script exits with code 2 and prints
   recovery instructions (see below). No partial state — failed patches don't
   leave the worktree in a half-applied condition because `--check` runs
   first.
5. **`docker build`** the worktree (which is now upstream/main + patches),
   producing two tags:
   - `makvitaly/appflowy_cloud:<upstream-version>-relations`
   - `makvitaly/appflowy_cloud:latest-relations`
   Cold build is 15–20 min. Subsequent runs with unchanged inputs are
   single-digit seconds (BuildKit cache hits every layer).
6. **Remove the worktree** (also runs on any error via `trap`).
7. **Print summary**: upstream version, commit, patches applied, image tags,
   ID, size.

The Docker build target is the existing production `Dockerfile` (multi-stage,
final `runtime` image is a debian-slim with the compiled binary). It does NOT
run `cargo test` or `cargo check --tests` — those need rocksdb compiled,
which OOMs a 4 GiB Colima VM. The release-bin build alone does not pull in
rocksdb (it's a dev-only dep through `client-api`).

## Usage

### Just refresh against current upstream

```bash
./scripts/build-patched.sh
```

### Override the image name / worktree location

```bash
APPFLOWY_BUILD_IMAGE_NAME=ghcr.io/makvitaly/appflowy_cloud \
APPFLOWY_BUILD_WORKTREE=/var/tmp/appflowy-build \
  ./scripts/build-patched.sh
```

### Inspect what would be done without running docker

Patches and worktree creation are cheap; comment out the `docker build` line
locally and dry-run. (The script does not yet have a `--dry-run` flag — add
one if it becomes useful.)

## When a patch fails to apply

This happens when upstream refactored a file the patch touches. Output looks
like:

```
==> Applying 001-expose-relation-field-in-row-detail.patch
!! Patch 001-expose-relation-field-in-row-detail.patch does not apply cleanly to upstream/main:
!!
    error: patch failed: src/api/workspace.rs:2739
    error: src/api/workspace.rs: patch does not apply
```

**The script does NOT auto-rebase.** Rebasing across a refactor is the kind
of merge that wants a human to look at it. The script just fails loudly so
you notice.

Recovery flow (manual, on the main checkout — not the worktree):

```bash
cd ~/dev/AppFlowy-Cloud

# Bring upstream up to date and switch to the branch the patches come from.
git fetch upstream
git checkout feat/relation-in-row-detail-api

# Replay the fork commits onto fresh upstream/main. This is where conflicts
# surface in your editor; resolve and 'git rebase --continue'.
git rebase upstream/main

# Re-export the patches from the now-fresh branch. --keep-subject preserves
# Conventional Commit prefixes; --no-numbered keeps git from rewriting the
# 0001-/0002- prefixes (we use our own numbering scheme).
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

# Re-run the script.
./scripts/build-patched.sh
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
