#!/usr/bin/env bash
# Reproducible runner for the OpenFlows Manager PostgreSQL integration tests.
#
# Starts an isolated `openflows-db` PostgreSQL (if not already running), waits
# for it to accept connections, and runs the manager integration tests against
# it. The Openflows control-plane database is separate from Coder's database.
#
# Usage:
#   ./crates/openflows-manager/scripts/run-integration-tests.sh
#
# Env overrides:
#   OPENFLOWS_TEST_DATABASE_URL  - full test database URL (defaults to the
#                                  bundled openflows-db on localhost:5544)
#   OPENFLOWS_PG_PORT            - host port for the bundled openflows-db
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
MANAGER_CRATE="$REPO_ROOT/crates/openflows-manager"
PORT="${OPENFLOWS_PG_PORT:-5544}"
TEST_DB_URL="${OPENFLOWS_TEST_DATABASE_URL:-postgres://openflows:openflows@localhost:${PORT}/openflows_control_plane}"

echo "==> Ensuring Openflows control-plane PostgreSQL is running on port ${PORT}"
# Start via the bundled compose profile. `--remove-orphans` avoids touching the
# unrelated Coder services. If the DB is already reachable, `up` is a no-op.
docker compose --profile manager up -d openflows-db 2>/dev/null || \
  docker compose --profile manager up -d openflows-db

echo "==> Waiting for PostgreSQL to accept connections on ${PORT}"
for i in $(seq 1 30); do
  if docker exec openflows-db pg_isready -U openflows -h localhost -p 5432 >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "==> Running manager tests with OPENFLOWS_TEST_DATABASE_URL=${TEST_DB_URL}"
# The live-PostgreSQL integration tests are `#[ignore]`d so plain `cargo test`
# / CI `nextest run` (which has no database) skip them; this runner explicitly
# opts in with `--ignored` now that a database is available.
(
  cd "$MANAGER_CRATE"
  OPENFLOWS_TEST_DATABASE_URL="$TEST_DB_URL" cargo test --test postgres --test ready --test health -- --ignored
)

echo "==> Done. Leave the DB running with: docker compose --profile manager up -d openflows-db"
echo "    Stop it with:                              docker compose --profile manager down"
