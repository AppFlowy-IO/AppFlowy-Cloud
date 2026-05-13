#!/usr/bin/env bash
# build-patched.sh — Refresh from upstream/main, apply local patches, build the
# patched appflowy_cloud Docker image.
#
# This script is intentionally narrow: it does NOT rebase the feat branch, NOT
# run tests, and NOT push the image. It only:
#
#   1. fetches upstream/main
#   2. spins up a disposable git worktree at upstream/main
#   3. applies every file under patches/*.patch in alphabetic order
#   4. docker build's the prod Dockerfile against that worktree
#   5. tags the result as makvitaly/appflowy_cloud:<upstream-version>-relations
#      + makvitaly/appflowy_cloud:latest-relations
#   6. removes the worktree
#
# When a patch no longer applies cleanly (upstream refactored the affected
# file), the script exits with code 2 and prints the conflict. The fix is a
# manual rebase of feat/relation-in-row-detail-api by the operator — see the
# README at scripts/build-patched.md for the recovery flow.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PATCHES_DIR="$REPO_ROOT/patches"
WORKTREE_DIR="${APPFLOWY_BUILD_WORKTREE:-/tmp/appflowy-build-patched}"
IMAGE_NAME="${APPFLOWY_BUILD_IMAGE_NAME:-makvitaly/appflowy_cloud}"

log() { printf "==> %s\n" "$*"; }
err() { printf "!! %s\n" "$*" >&2; }

cleanup_worktree() {
  if [[ -d "$WORKTREE_DIR" ]]; then
    git -C "$REPO_ROOT" worktree remove --force "$WORKTREE_DIR" 2>/dev/null || rm -rf "$WORKTREE_DIR"
  fi
}
trap cleanup_worktree EXIT

# ---------- 1. Fetch upstream/main ----------
cd "$REPO_ROOT"
log "Fetching upstream/main"
git fetch upstream main --quiet

UPSTREAM_SHA="$(git rev-parse upstream/main)"
UPSTREAM_SHORT="$(git rev-parse --short upstream/main)"

# Prefer the nearest annotated tag (AppFlowy-Cloud tags releases as plain
# semver e.g. "0.9.64"). Fall back to short SHA if upstream/main is ahead of
# every tag.
if UPSTREAM_VERSION="$(git describe --tags --abbrev=0 upstream/main 2>/dev/null)"; then
  UPSTREAM_VERSION="${UPSTREAM_VERSION#v}"  # strip "v" if any future tag uses it
else
  UPSTREAM_VERSION="git-${UPSTREAM_SHORT}"
fi

log "Upstream version: $UPSTREAM_VERSION ($UPSTREAM_SHORT)"

# ---------- 2. List patches ----------
shopt -s nullglob
PATCHES=( "$PATCHES_DIR"/*.patch )
shopt -u nullglob

if [[ ${#PATCHES[@]} -eq 0 ]]; then
  err "No patches found in $PATCHES_DIR — nothing to apply, refusing to build a"
  err "vanilla upstream image under this script (use 'docker build' directly)."
  exit 1
fi

log "Patches to apply (${#PATCHES[@]}):"
for p in "${PATCHES[@]}"; do
  printf "    %s\n" "$(basename "$p")"
done

# ---------- 3. Prepare disposable worktree ----------
cleanup_worktree  # remove leftover from earlier failed run

log "Creating worktree at $WORKTREE_DIR (detached at $UPSTREAM_SHORT)"
git worktree add --detach "$WORKTREE_DIR" "$UPSTREAM_SHA" >/dev/null

# ---------- 4. Apply patches ----------
for patch in "${PATCHES[@]}"; do
  name="$(basename "$patch")"
  log "Applying $name"
  if ! git -C "$WORKTREE_DIR" apply --check "$patch" 2>"$WORKTREE_DIR/.patch-error"; then
    err "Patch $name does not apply cleanly to upstream/main:"
    err ""
    sed 's/^/    /' "$WORKTREE_DIR/.patch-error" >&2
    err ""
    err "Recovery (manual, on the main checkout):"
    err "  1. cd $REPO_ROOT"
    err "  2. git fetch upstream"
    err "  3. git checkout feat/relation-in-row-detail-api"
    err "  4. git rebase upstream/main"
    err "  5. resolve conflicts in your editor, then 'git rebase --continue'"
    err "  6. git format-patch upstream/main..feat/relation-in-row-detail-api \\"
    err "       -o patches/"
    err "  7. rename to NNN-<slug>.patch matching the existing files (overwrite)"
    err "  8. git push --force-with-lease origin feat/relation-in-row-detail-api"
    err "  9. re-run ./scripts/build-patched.sh"
    exit 2
  fi
  git -C "$WORKTREE_DIR" apply "$patch"
done

# ---------- 5. Build ----------
TAG_VERSIONED="${IMAGE_NAME}:${UPSTREAM_VERSION}-relations"
TAG_LATEST="${IMAGE_NAME}:latest-relations"

log "Building $TAG_VERSIONED"
log "(also tagging $TAG_LATEST)"

# BuildKit caches layers in dockerd. With identical inputs every layer is a
# CACHED hit and this finishes in single-digit seconds. When only upstream/main
# moves but Cargo.toml/Cargo.lock are unchanged, the cargo-chef cook layer
# still cache-hits and only the final 'cargo build --release --bin
# appflowy_cloud' re-runs (~2 min). Cold build (fresh dockerd cache) is
# 15-20 min.
docker build \
  --tag "$TAG_VERSIONED" \
  --tag "$TAG_LATEST" \
  "$WORKTREE_DIR"

# ---------- 6. Summary ----------
# `docker image inspect`'s Size is the descriptor blob size for buildx's
# multi-platform manifest list, not the assembled image. `docker images`
# reports the actual on-disk size — use that.
SIZE="$(docker images --format '{{.Size}}' "$TAG_VERSIONED" | head -1)"
DIGEST="$(docker images --format '{{.ID}}' "$TAG_VERSIONED" | head -1)"

cat <<EOF

==> Build complete
    Upstream version : $UPSTREAM_VERSION
    Upstream commit  : $UPSTREAM_SHORT
    Patches applied  : ${#PATCHES[@]}
    Image tags       : $TAG_VERSIONED
                       $TAG_LATEST
    Image ID         : $DIGEST
    Image size       : $SIZE

Next step (operator):
    Update image: line or APPFLOWY_CLOUD_VERSION in
    ~/server/stacks/appflowy/docker-compose.yml to point at $TAG_VERSIONED,
    then 'docker compose up -d --force-recreate appflowy_cloud'.
EOF
