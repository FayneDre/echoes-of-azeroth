#!/usr/bin/env bash
# Prints the id of a workflow run whose artifact <name> can be reused instead of
# compiling, or nothing. Requires `gh` (GH_TOKEN) and GITHUB_REPOSITORY.
#
# Trust rules (an artifact name is public and predictable, so the name alone
# proves nothing):
# - produced by this repository's build-server workflow (not ci.yml, not a fork),
# - by a `push` or `workflow_dispatch` run on main (never a pull request),
# - not expiring within 2 days (it must survive until the smoke test downloads it).
# Any API error means "no artifact": the caller then compiles, which is always safe.
#
# Usage: tools/ci/find-artifact.sh <artifact-name>
set -uo pipefail

name="${1:?artifact name}"
repo="${GITHUB_REPOSITORY:?}"
workflow_path=".github/workflows/build-server.yml"
min_expiry="$(date -u -d '+2 days' +%Y-%m-%dT%H:%M:%SZ)"

candidates="$(gh api "repos/$repo/actions/artifacts?name=$name&per_page=100" --jq \
  "[.artifacts[] | select(.expired == false and .expires_at > \"$min_expiry\")]
   | sort_by(.created_at) | reverse | .[].workflow_run.id" 2>/dev/null)" || {
  echo "::warning::artifact lookup failed, compiling instead" >&2
  exit 0
}

for run_id in $candidates; do
  verdict="$(gh api "repos/$repo/actions/runs/$run_id" --jq \
    "if .path == \"$workflow_path\"
        and (.event == \"push\" or .event == \"workflow_dispatch\")
        and .head_branch == \"main\"
        and .head_repository.full_name == \"$repo\"
        and .repository.full_name == \"$repo\"
     then \"trusted\" else \"rejected: \(.path) \(.event) \(.head_branch) \(.head_repository.full_name)\" end" 2>/dev/null)" || continue
  if [ "$verdict" = "trusted" ]; then
    echo "$run_id"
    exit 0
  fi
  echo "run $run_id $verdict" >&2
done
