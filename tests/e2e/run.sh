#!/usr/bin/env bash
# Run only against a new, disposable Docker stack; no production credentials.
set -euo pipefail
cd "$(dirname "$0")/../.."
project="openflows-ci-$(date +%s)-$$"
artifacts="$PWD/target/ci-artifacts/coder"
mkdir -p "$artifacts"
scratch=$(mktemp -d)
compose=(docker compose --env-file /dev/null -p "$project" -f tests/e2e/compose.yml)
cleanup() {
    result=$?
    trap - EXIT
    "${compose[@]}" logs --no-color >"$artifacts/stack.log" 2>&1 || true
    while IFS= read -r id; do
        [ -n "$id" ] || continue
        docker logs "$id" >"$artifacts/worker-$id.log" 2>&1 || true
        docker rm -f "$id" >/dev/null 2>&1 || result=1
    done < <(docker ps -aq --filter "label=openflows.ci.project=$project")
    "${compose[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || result=1
    docker image rm "$project-worker:ci" >/dev/null 2>&1 || true
    rm -rf "$scratch"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

cargo build --locked -p openflows-harness --bin openflows-harness 2>&1 | tee "$artifacts/build.log"
cp target/debug/openflows-harness "$scratch/"
cp tests/e2e/worker.Dockerfile "$scratch/Dockerfile"
docker build -t "$project-worker:ci" "$scratch" 2>&1 | tee "$artifacts/image.log"
"${compose[@]}" up -d --wait --wait-timeout 120
coder_id=$("${compose[@]}" ps -q coder)
# Use the exact CLI shipped in the pinned server image.
docker cp "$coder_id:/opt/coder" "$scratch/coder"
chmod +x "$scratch/coder"
export PATH="$scratch:$PATH"
export CODER_CONFIG_DIR="$scratch/config"
export OPENFLOWS_E2E_CODER_URL="http://$("${compose[@]}" port coder 7080)"
export TEST_REDIS_URL="redis://$("${compose[@]}" port redis 6379)"
export TF_VAR_dev_binary_host_path="$project"
tar -czf "$scratch/template.tar.gz" -C tests/e2e/template .
export OPENFLOWS_E2E_TEMPLATE_ARCHIVE="$scratch/template.tar.gz"
timeout 1200 cargo test --locked -p coder-client --test container_e2e -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "$artifacts/tests.log"
