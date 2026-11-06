#!/usr/bin/env bash
# Fails while SonarCloud holds an open issue on this pull request, or on main
# for a push to it. It waits for the analysis of COMMIT to finish, so
# what it reads is about that commit. An analysis that never reports fails too:
# a check that read nothing has not passed.
set -euo pipefail

: "${PROJECT:?PROJECT is the SonarCloud project}"
: "${COMMIT:?COMMIT is the commit analysed}"
: "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY names the repository}"

check="SonarCloud Code Analysis"
tries=${SONAR_TRIES:-90}
pause=${SONAR_PAUSE:-20}

finished=""
for ((try = 0; try < tries; try++)); do
  finished=$(gh api "repos/${GITHUB_REPOSITORY}/commits/${COMMIT}/check-runs?check_name=${check// /%20}" \
    --jq '[.check_runs[] | select(.status == "completed")] | length')
  if [[ "${finished}" -gt 0 ]]; then
    break
  fi
  sleep "${pause}"
done
if [[ "${finished:-0}" -eq 0 ]]; then
  echo "::error::SonarCloud did not finish analysing ${COMMIT}."
  exit 1
fi

if [[ -n "${PULL_REQUEST:-}" ]]; then
  scope="pullRequest=${PULL_REQUEST}"
else
  scope="branch=main"
fi
url="https://sonarcloud.io/api/issues/search?componentKeys=${PROJECT}&${scope}&resolved=false&ps=100"
answer=$(curl --fail --silent --show-error --max-time 30 "${url}")
open=$(jq '.paging.total' <<<"${answer}")
if [[ "${open}" -eq 0 ]]; then
  echo "SonarCloud holds no open issue (${scope})."
  exit 0
fi
jq -r '.issues[] | "::error file=\(.component | split(":") | last),line=\(.line // 1)::\(.rule): \(.message)"' <<<"${answer}"
echo "::error::SonarCloud holds ${open} open issues (${scope})."
exit 1
