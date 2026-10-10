#!/usr/bin/env bash
# Runs the first production-bootstrap E2E scenario from a fresh checkout.
# Builds the actual OpenFlows controller and harness from this revision.
# Creates a private Docker stack with real Coder, PostgreSQL, and Redis.
# Starts local HTTP fixtures for OAuth, GitHub, and the model provider.
# Uses the five bundled production Terraform templates during tenant setup.
# Provides an isolated OpenFlows directory and Coder CLI configuration.
# Calls the public tenant CLI with one FORGE and SENTINEL pair.
# Checks that the real Nexus controller and Coder chat can run commands.
# Saves service journals and workspace logs for diagnosing failures.
# Removes resources on this run's private network when the scenario exits.
# This bootstrap scenario does not yet prove the entire issue-to-merge flow.

# Public tenant bootstrap against fresh services and the bundled production templates.
set -euo pipefail
cd "$(dirname "$0")/../../.."
project="openflows-bootstrap-$(date +%s)-$$"
artifacts="$PWD/target/ci-artifacts/production-bootstrap/$project"
mkdir -p "$artifacts"
scratch=$(mktemp -d)
export OPENFLOWS_SYSTEM_FIXTURE_IMAGE="$project-fixtures:ci"
compose=(docker compose --env-file /dev/null -p "$project" -f "$PWD/tests/e2e/compose.yml" -f "$PWD/tests/e2e/system/bootstrap.compose.yml")
cleanup() {
    result=$?
    trap - EXIT
    "${compose[@]}" logs --no-color >"$artifacts/stack.log" 2>&1 || true
    # Only this run's private network can contain these resources.
    while IFS= read -r id; do
        [ -n "$id" ] || continue
        docker logs "$id" >"$artifacts/container-$id.log" 2>&1 || true
        docker exec "$id" cat /tmp/openflows-controller.log >"$artifacts/controller-$id.log" 2>&1 || true
        docker cp "$id:/tmp/oauth/oauth-requests.jsonl" "$artifacts/oauth-requests.jsonl" 2>/dev/null || true
        docker cp "$id:/tmp/model/model-requests.jsonl" "$artifacts/model-requests.jsonl" 2>/dev/null || true
        docker cp "$id:/tmp/github/github-requests.jsonl" "$artifacts/github-requests.jsonl" 2>/dev/null || true
        docker inspect "$id" --format '{{range .Mounts}}{{if eq .Type "volume"}}{{println .Name}}{{end}}{{end}}' >>"$scratch/volumes" || true
        docker rm -f "$id" >/dev/null 2>&1 || result=1
    done < <(docker ps -aq --filter "network=${project}_default")
    "${compose[@]}" down --volumes --remove-orphans >"$artifacts/cleanup.log" 2>&1 || result=1
    if [ -f "$scratch/volumes" ]; then
        while IFS= read -r volume; do
            [ -n "$volume" ] || continue
            docker volume rm "$volume" >/dev/null 2>&1 || true
        done < <(sort -u "$scratch/volumes")
    fi
    docker image rm "$project-workspace:ci" "$OPENFLOWS_SYSTEM_FIXTURE_IMAGE" >/dev/null 2>&1 || true
    rm -rf "$scratch"
    exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cargo build --locked -p openflows --bin openflows -p openflows-harness --bin openflows-harness 2>&1 | tee "$artifacts/build.log"
mkdir -p "$scratch/bin" "$scratch/home"
cp target/debug/openflows target/debug/openflows-harness "$scratch/bin/"
docker build -t "$OPENFLOWS_SYSTEM_FIXTURE_IMAGE" tests/e2e/system 2>&1 | tee "$artifacts/fixtures-image.log"
docker build -t "$project-workspace:ci" -f tests/e2e/system/workspace.Dockerfile tests/e2e/system 2>&1 | tee "$artifacts/workspace-image.log"
"${compose[@]}" up -d --wait --wait-timeout 120
coder_id=$("${compose[@]}" ps -q coder)
docker cp "$coder_id:/opt/coder" "$scratch/bin/coder"
chmod +x "$scratch/bin/coder"
export PATH="$scratch/bin:$PATH"
export OPENFLOWS_HOME="$scratch/home/.openflows"
unset OPENFLOWS_REGISTRY_PATH OPENFLOWS_REGISTRY_JSON
export RUST_LOG=info
export CODER_CONFIG_DIR="$scratch/home/coder"
export CODER_URL="http://$("${compose[@]}" port coder 7080)"
export CODER_ADMIN_EMAIL=ci@example.test
export CODER_ADMIN_USERNAME=ci
export CODER_ADMIN_PASSWORD=Disposable-CI-password-729!
export REDIS_URL="redis://$("${compose[@]}" port redis 6379)"
export CODER_CHAT_HOOK_SECRET=disposable-ci-hook-secret-at-least-32-bytes
export CODER_CHAT_HOOK_URL=http://openflows-nexus:3001/experimental/hooks/chat
export GITHUB_GIT_BASE=git://github-fixture:9418
export TF_VAR_dev_binary_host_path="$scratch/bin"
export TF_VAR_docker_network="${project}_default"
export TF_VAR_workspace_image="$project-workspace:ci"
export TF_VAR_github_api_base=http://github-fixture:8080
export TF_VAR_github_git_base="$GITHUB_GIT_BASE"
export OPENFLOWS_E2E_OAUTH_URL="http://$("${compose[@]}" port oauth 8080)"
export OPENFLOWS_E2E_ARTIFACTS="$artifacts"
# Keep the prefixed Nexus workspace name within Coder's 32-character limit.
export OPENFLOWS_E2E_TENANT="ci-$(printf '%s' "$project" | sha256sum | cut -c1-12)"
driver="$PWD/tests/e2e/system/production_bootstrap.py"
# Avoid loading the developer's .env or persisted bootstrap credentials.
cd "$scratch"
timeout 900 python3 "$driver" 2>&1 | tee "$artifacts/test.log"
