#!/usr/bin/env bash
# Fails while SonarCloud holds an open issue on this pull request, or on main
# for a push to it. It waits until SonarCloud's latest analysis of that pull
# request or branch is of COMMIT itself, so what it reads is about that
# commit and not one pushed before it. An analysis that never reports fails
# too: a check that read nothing has not passed.
set -euo pipefail

: "${PROJECT:?PROJECT is the SonarCloud project}"
: "${COMMIT:?COMMIT is the commit analysed}"

tries=${SONAR_TRIES:-90}
pause=${SONAR_PAUSE:-20}

if [[ -n "${PULL_REQUEST:-}" ]]; then
  scope="pullRequest=${PULL_REQUEST}"
  analysed="api/project_pull_requests/list?project=${PROJECT}"
  latest=".pullRequests[] | select(.key == \"${PULL_REQUEST}\") | .commit.sha"
else
  scope="branch=main"
  analysed="api/project_branches/list?project=${PROJECT}"
  latest='.branches[] | select(.isMain) | .commit.sha'
fi

sha=""
for ((try = 0; try < tries; try++)); do
  sha=$(curl --fail --silent --show-error --max-time 30 "https://sonarcloud.io/${analysed}" \
    | jq -r "${latest}" || true)
  if [[ "${sha}" == "${COMMIT}" ]]; then
    break
  fi
  sleep "${pause}"
done
if [[ "${sha}" != "${COMMIT}" ]]; then
  echo "::error::SonarCloud did not finish analysing ${COMMIT}; its latest analysis (${scope}) is of ${sha:-nothing}."
  exit 1
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
