#!/usr/bin/env bash
# The steps of .github/workflows/release.yml. Run from the repository root:
#
#     bash .github/scripts/release.sh prepare
#     bash .github/scripts/release.sh published VERSION
#     bash .github/scripts/release.sh tag TAG COMMIT
#
# Results are printed as key=value lines and, in Actions, appended to $GITHUB_OUTPUT. Release
# tags are looked up on origin.
set -euo pipefail

CRATE=substrate-metadata-mcp
# Crates of four letters and more sit under their first two and next two letters.
INDEX=https://index.crates.io/su/bs/$CRATE

usage() {
    sed -n 's/^#     //p' "$0" >&2
    exit 2
}

fail() {
    echo "::error::$1"
    exit 1
}

set_outputs() {
    printf '%s\n' "$@"
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
        printf '%s\n' "$@" >> "$GITHUB_OUTPUT"
    fi
}

# Whether origin has the tag. Origin is asked directly: a local tag may be stale or not pushed.
# With --exit-code, git exits with 2 when it reached origin and found no such tag; any other
# failure means the answer is unknown, so the run fails instead of guessing.
tag_exists() {
    local status=0
    git ls-remote --exit-code --tags origin "refs/tags/$1" > /dev/null || status=$?
    case $status in
        0) return 0 ;;
        2) return 1 ;;
        *) fail "git ls-remote origin refs/tags/$1 failed" ;;
    esac
}

# Decides whether this run releases, and prints what the later jobs need.
#
# The version is the package version in Cargo.toml, and tag v<version> marks it released.
# Without the tag it is released from the commit that set it, and only by a run on that commit
# (the release PR's merge, or its dry run) or by a manual one. A later push only gets a warning,
# so a failed release is finished on purpose, never as a side effect of merging something else.
prepare() {
    local version tag latest release=false commit=""

    version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
    [ -n "$version" ] || fail "no package version in Cargo.toml"
    tag="v$version"

    # crates.io takes any version not published yet, a lower one too, so a typo such as 0.0.1
    # would go out unless the version is checked against the latest release. The digit after the v
    # keeps out other tags on v: sort -V would put them above every release.
    latest=$(git ls-remote --tags --refs origin 'v[0-9]*' | sed 's|.*refs/tags/||' | sort -V | tail -n 1)
    if [ -n "$latest" ] && [ "$latest" != "$tag" ] &&
        [ "$(printf '%s\n' "$latest" "$tag" | sort -V | tail -n 1)" != "$tag" ]; then
        fail "version $version is below the latest release $latest"
    fi

    if tag_exists "$tag"; then
        echo "$version is released already as $tag"
    else
        # -S looks for a change in how many lines match, so a commit that only moves the version
        # line doesn't count as setting it, as it would with -G. The anchors keep a dependency of
        # the same version from matching. With --first-parent, a merge commit counts as setting
        # what its branch set, e.g. the merge commit of a PR.
        commit=$(git log -1 --first-parent --format=%H --pickaxe-regex \
            -S "^version = \"${version//./\\.}\"\$" -- Cargo.toml)
        [ -n "$commit" ] || fail "no commit sets version $version in Cargo.toml"

        if [ "${GITHUB_EVENT_NAME:-}" = workflow_dispatch ] || [ "$commit" = "$(git rev-parse HEAD)" ]; then
            release=true
            if [ "${GITHUB_EVENT_NAME:-}" = pull_request ]; then
                echo "::notice::Merging this PR releases $version."
            fi
        else
            echo "::warning::$version isn't released yet (no $tag), and this run doesn't release it." \
                "If its release run failed, re-run it, or run Release on main by hand."
            commit=""
        fi
    fi

    set_outputs "should-release=$release" "version=$version" "tag=$tag" "commit=$commit"
}

# Whether crates.io has the version already. It takes a version only once, so a re-run after a
# partial success has to skip publishing to get to the tag. The sparse index is a static file;
# the crates.io API would ask for a User-Agent.
published() {
    local version=$1 index
    index=$(curl --fail --silent --show-error "$INDEX")

    if grep --quiet --fixed-strings "\"vers\":\"$version\"" <<< "$index"; then
        echo "::notice::$version is on crates.io already."
        set_outputs "published=true"
    else
        set_outputs "published=false"
    fi
}

# Puts a lightweight tag on the commit through the GitHub API; needs GH_TOKEN and
# GITHUB_REPOSITORY. A re-run may find the tag made already: it must point to the same commit.
# Tags made with GITHUB_TOKEN trigger no other workflows.
create_tag() {
    local tag=$1 commit=$2 existing

    if ! gh api "repos/$GITHUB_REPOSITORY/git/refs" -f "ref=refs/tags/$tag" -f "sha=$commit" > /dev/null; then
        existing=$(gh api "repos/$GITHUB_REPOSITORY/git/ref/tags/$tag" --jq .object.sha)
        [ "$existing" = "$commit" ] || fail "$tag points to $existing, not to $commit"
    fi

    echo "::notice::Released $tag from $commit."
}

case "${1:-}" in
    prepare) [ $# -eq 1 ] || usage; prepare ;;
    published) [ $# -eq 2 ] || usage; published "$2" ;;
    tag) [ $# -eq 3 ] || usage; create_tag "$2" "$3" ;;
    *) usage ;;
esac
