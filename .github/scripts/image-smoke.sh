#!/usr/bin/env bash
# Runs the image built as herald:ci alone, under Compose with SQLite and with
# Postgres, and on a kind cluster from the manifests and from the chart, and
# fails unless herald answers healthy in each.
set -euo pipefail

image="${IMAGE:-herald:ci}"
work="$(mktemp -d)"
trap 'docker rm -fv herald-smoke >/dev/null 2>&1 || true; rm -rf "${work}"' EXIT

healthy() {
  local container="$1" status
  for _ in $(seq 1 60); do
    status="$(docker inspect --format '{{.State.Health.Status}}' "${container}")"
    [ "${status}" = "healthy" ] && return 0
    sleep 2
  done
  docker logs "${container}" >&2
  echo "${container} is ${status}, not healthy" >&2
  return 1
}

secret="$(head -c 32 /dev/urandom | base64 | tr -d '\n')"
mkdir -p "${work}/secrets"
printf '%s\n' "${secret}" > "${work}/secrets/HERALD_MARKER_SECRET"
cp deploy/compose/herald.toml "${work}/herald.toml"

echo "The image alone"
docker run -d --name herald-smoke \
  -v "${work}/herald.toml:/etc/herald/herald.toml:ro" \
  -v "${work}/secrets:/run/secrets:ro" \
  --read-only "${image}"
healthy herald-smoke
[ "$(docker exec herald-smoke herald health)" = "herald answers at http://127.0.0.1:8080/healthz" ]
[ "$(docker inspect --format '{{.Config.User}}' "${image}")" = "65532:65532" ]
docker rm -fv herald-smoke >/dev/null

echo "Compose with SQLite, then with Postgres"
cp deploy/compose/*.yaml deploy/compose/*.toml "${work}/"
password="$(head -c 24 /dev/urandom | base64 | tr -d '/+=\n')"
printf '%s\n' "${password}" > "${work}/secrets/POSTGRES_PASSWORD"
printf 'postgres://herald:%s@postgres/herald?sslmode=disable\n' "${password}" > "${work}/secrets/HERALD_POSTGRES_URL"
(
  cd "${work}"
  export HERALD_IMAGE="${image}"
  docker compose -f compose.yaml up -d --wait --wait-timeout 120
  docker compose -f compose.yaml down -v
  docker compose -f compose.yaml -f compose.postgres.yaml up -d --wait --wait-timeout 180
  docker compose -f compose.yaml -f compose.postgres.yaml exec -T herald herald health
  docker compose -f compose.yaml -f compose.postgres.yaml down -v
)

echo "Kubernetes manifests and the Helm chart"
kubectl kustomize deploy/kubernetes | kubeconform -strict -summary
helm lint --strict deploy/helm/herald
helm template herald deploy/helm/herald | kubeconform -strict -summary
kind create cluster --name herald-smoke --wait 120s
trap 'kind delete cluster --name herald-smoke >/dev/null 2>&1 || true; rm -rf "${work}"' EXIT
kind load docker-image "${image}" --name herald-smoke
mkdir -p "${work}/overlay"
cp -r deploy/kubernetes "${work}/base"
cat > "${work}/overlay/kustomization.yaml" <<YAML
apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - ../base
images:
  - name: ghcr.io/nightworksio/herald
    newName: ${image%%:*}
    newTag: ${image##*:}
YAML
kubectl create secret generic herald-secrets --from-file=HERALD_MARKER_SECRET="${work}/secrets/HERALD_MARKER_SECRET"
kubectl apply -k "${work}/overlay"
kubectl rollout status deployment/herald --timeout=180s
kubectl delete -k "${work}/overlay" --wait
helm install herald deploy/helm/herald --wait --timeout 3m \
  --set image.repository="${image%%:*}" --set image.tag="${image##*:}" --set image.pullPolicy=Never \
  --set secrets.existingSecret=herald-secrets --set server.jobs=4 --set schedule.poll=1h
kubectl rollout status deployment/herald --timeout=180s
echo "herald answered healthy in every one"
