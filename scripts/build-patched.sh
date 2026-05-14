#!/usr/bin/env bash
# build-patched.sh — Resolve a base git ref on the upstream remote, apply
# local patches, and build the patched appflowy_cloud Docker image.
#
# Defaults to `--base main` (upstream/main) for the historical behavior.
# Useful overrides:
#   --base v0.15.17        # try a published tag
#   --base release-0.16    # try a release branch on upstream
#   --base abc1234         # try a specific commit SHA
#
# The script is intentionally narrow: it does NOT rebase the feat branch, NOT
# run tests, and NOT push the image. It only:
#
#   1. fetches the upstream remote
#   2. resolves --base into a concrete commit SHA on upstream
#   3. spins up a disposable git worktree detached at that SHA
#   4. applies every file under patches/*.patch in alphabetic order
#   5. docker build's the prod Dockerfile against that worktree
#   6. tags the result as makvitaly/appflowy_cloud:<base-tag>-relations
#      + makvitaly/appflowy_cloud:latest-relations
#   7. removes the worktree
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

usage() {
  cat <<EOF
Usage: $(basename "$0") [--base <git-ref>] [--help]

  --base <git-ref>   Resolve <git-ref> as the base for the patched build.
                     Tried in order on the local repo (after fetching
                     upstream):
                       1. upstream/<ref>  (branch on upstream remote)
                       2. refs/tags/<ref> (tag)
                       3. <ref>           (commit SHA / any ref)
                     Default: main
  -h, --help         Show this help

Environment:
  APPFLOWY_BUILD_IMAGE_NAME   Override the image repo name.
                              Default: makvitaly/appflowy_cloud
  APPFLOWY_BUILD_WORKTREE     Override the worktree path.
                              Default: /tmp/appflowy-build-patched

Examples:
  $(basename "$0")
      Build from upstream/main, tag :<upstream-version>-relations.

  $(basename "$0") --base v0.15.17
      Build from the v0.15.17 tag.

  $(basename "$0") --base release-0.16
      Build from the release-0.16 branch on upstream.

  $(basename "$0") --base abc1234
      Build from a specific commit SHA.
EOF
}

# ---------- 0. Parse args ----------
BASE="main"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --base)
      [[ -z "${2:-}" ]] && { err "--base needs a value"; usage >&2; exit 1; }
      BASE="$2"
      shift 2
      ;;
    --base=*)
      BASE="${1#--base=}"
      [[ -z "$BASE" ]] && { err "--base needs a value"; usage >&2; exit 1; }
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      err "Unknown argument: $1"
      usage >&2
      exit 1
      ;;
  esac
done

cleanup_worktree() {
  if [[ -d "$WORKTREE_DIR" ]]; then
    git -C "$REPO_ROOT" worktree remove --force "$WORKTREE_DIR" 2>/dev/null || rm -rf "$WORKTREE_DIR"
  fi
}
trap cleanup_worktree EXIT

# ---------- 1. Fetch upstream ----------
cd "$REPO_ROOT"
log "Fetching upstream"
git fetch upstream --tags --quiet

# ---------- 2. Resolve --base ----------
log "Resolving --base '$BASE'"

RESOLVED_SHA=""
RESOLVED_REF=""
RESOLVED_KIND=""
for entry in "branch:upstream/$BASE" "tag:refs/tags/$BASE" "commit:$BASE"; do
  kind="${entry%%:*}"
  ref="${entry#*:}"
  if sha="$(git rev-parse --verify "${ref}^{commit}" 2>/dev/null)"; then
    RESOLVED_SHA="$sha"
    RESOLVED_REF="$ref"
    RESOLVED_KIND="$kind"
    break
  fi
done

if [[ -z "$RESOLVED_SHA" ]]; then
  err "Could not resolve --base '$BASE'. Tried:"
  err "    upstream/$BASE  (branch on upstream remote)"
  err "    refs/tags/$BASE (tag)"
  err "    $BASE           (commit SHA / arbitrary ref)"
  err ""
  err "Check:"
  err "    git branch -r           # for upstream branches"
  err "    git tag --list          # for tags"
  err "    git rev-parse <sha>     # for commit hashes"
  exit 1
fi

RESOLVED_SHORT="$(git rev-parse --short "$RESOLVED_SHA")"
log "Resolved $RESOLVED_KIND $RESOLVED_REF -> $RESOLVED_SHORT"

# ---------- 3. Decide image tag ----------
# For the default `--base main`, the historical tag scheme is the nearest
# semver tag of upstream (e.g. "0.9.64-relations"). For an explicit
# --base, use the ref name as-is, sanitized for docker-tag legality
# ([a-zA-Z0-9._-]). Leading `v` is stripped so `v0.15.17` becomes
# `0.15.17-relations` for consistency.
if [[ "$BASE" == "main" ]]; then
  if BASE_VERSION="$(git describe --tags --abbrev=0 "$RESOLVED_SHA" 2>/dev/null)"; then
    BASE_VERSION="${BASE_VERSION#v}"
  else
    BASE_VERSION="git-${RESOLVED_SHORT}"
  fi
else
  BASE_VERSION="$(printf '%s' "$BASE" | tr -c 'a-zA-Z0-9._-' '-')"
  BASE_VERSION="${BASE_VERSION#v}"
fi

# ---------- 4. List patches ----------
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

# ---------- 5. Prepare disposable worktree ----------
cleanup_worktree  # remove leftover from earlier failed run

log "Creating worktree at $WORKTREE_DIR (detached at $RESOLVED_SHORT)"
git worktree add --detach "$WORKTREE_DIR" "$RESOLVED_SHA" >/dev/null

# ---------- 6. Apply patches ----------
for patch in "${PATCHES[@]}"; do
  name="$(basename "$patch")"
  log "Applying $name"
  if ! git -C "$WORKTREE_DIR" apply --check "$patch" 2>"$WORKTREE_DIR/.patch-error"; then
    err "Patch $name does not apply cleanly to $RESOLVED_REF ($RESOLVED_SHORT):"
    err ""
    sed 's/^/    /' "$WORKTREE_DIR/.patch-error" >&2
    err ""
    err "Recovery (manual, on the main checkout):"
    err "  1. cd $REPO_ROOT"
    err "  2. git fetch upstream"
    err "  3. git checkout feat/relation-in-row-detail-api"
    err "  4. git rebase $RESOLVED_REF"
    err "  5. resolve conflicts in your editor, then 'git rebase --continue'"
    err "  6. git format-patch $RESOLVED_REF..feat/relation-in-row-detail-api \\"
    err "       -o patches/"
    err "  7. rename to NNN-<slug>.patch matching the existing files (overwrite)"
    err "  8. git push --force-with-lease origin feat/relation-in-row-detail-api"
    err "  9. re-run ./scripts/build-patched.sh --base $BASE"
    exit 2
  fi
  git -C "$WORKTREE_DIR" apply "$patch"
done

# ---------- 7. Build ----------
TAG_VERSIONED="${IMAGE_NAME}:${BASE_VERSION}-relations"
TAG_LATEST="${IMAGE_NAME}:latest-relations"

log "Building $TAG_VERSIONED"
log "(also tagging $TAG_LATEST)"

# BuildKit caches layers in dockerd. With identical inputs every layer is a
# CACHED hit and this finishes in single-digit seconds. When only upstream
# moves but Cargo.toml/Cargo.lock are unchanged, the cargo-chef cook layer
# still cache-hits and only the final 'cargo build --release --bin
# appflowy_cloud' re-runs (~2 min). Cold build (fresh dockerd cache) is
# 15-20 min.
docker build \
  --tag "$TAG_VERSIONED" \
  --tag "$TAG_LATEST" \
  "$WORKTREE_DIR"

# ---------- 8. Summary ----------
# `docker image inspect`'s Size is the descriptor blob size for buildx's
# multi-platform manifest list, not the assembled image. `docker images`
# reports the actual on-disk size — use that.
SIZE="$(docker images --format '{{.Size}}' "$TAG_VERSIONED" | head -1)"
DIGEST="$(docker images --format '{{.ID}}' "$TAG_VERSIONED" | head -1)"

cat <<EOF

==> Build complete
    Base ref         : $RESOLVED_REF ($RESOLVED_KIND)
    Base commit      : $RESOLVED_SHORT
    Image tag suffix : $BASE_VERSION
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
