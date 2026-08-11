# OpenFlows — Quick Start

This guide covers everything you need to get OpenFlows running on a fresh machine: prerequisites, one-time setup (`.env`, Docker, bootstrap, licenses, tokens), adding a tenant, running the controller, verifying it works, and common troubleshooting.

> **Overview:** For what OpenFlows is, its architecture, how far it has come, and what's left to finish, see the [README](README.md).

---

## Prerequisites

- **Docker 24+** — runs Redis, the Coder database, and Coder itself.
- **Rust 1.70+** — builds the `openflows` and `openflows-harness` binaries during bootstrap.
- **Node 18+** — for the GitHub MCP tooling used by agents.
- **The `coder` CLI** on your `PATH` — `prod.sh bootstrap` shells out to `coder templates push`. Install it if missing:
  ```bash
  curl -fsSL https://coder.com/install.sh | sh
  ```
  Then make sure `coder` is on your `PATH` (re-login or add `~/.local/bin` / `~/bin`) and confirm with `coder version`.
- **A GitHub personal access token** with the `repo` scope.

---

## Step 1 — Configure the environment

```bash
cp .env.example .env
```

Edit `.env` and fill in at least:

| Variable | Description |
|----------|-------------|
| `GITHUB_TOKEN` | GitHub PAT with the `repo` scope (from <https://github.com/settings/tokens>) |
| `GITHUB_REPOSITORY` | Your repo as `owner/repo` (e.g. `my-org/my-repo`) |
| `CODER_SESSION_TOKEN` | Leave empty for now — obtained in [Step 4](#step-4--coder-license--github-login--api-token) |

Optional overrides (used when bootstrap creates the Coder admin account):

```bash
CODER_ADMIN_USERNAME=admin
CODER_ADMIN_EMAIL=admin@openflows.dev
CODER_ADMIN_PASSWORD=Op3nFl0ws!
```

> **`.dev-binaries` note:** This directory is created and populated automatically during bootstrap and is bind-mounted into Coder workspaces. If bootstrap fails with `cp: ...: Permission denied`, the directory has become `root`-owned. Fix it:
> ```bash
> sudo chown -R "$USER":"$USER" .dev-binaries/
> ```
> (Create it first if needed: `mkdir -p .dev-binaries`.)

---

## Step 2 — Start the Docker infrastructure

```bash
docker compose up -d
```

This starts three services (see `docker-compose.yml`):

- **Redis** — the shared state store (port `6379`)
- **coder-db** — PostgreSQL for Coder (no external port)
- **coder** — the Coder server itself (port `7080`)

Wait until all services are healthy:

```bash
docker compose ps
```

You should see `redis`, `coder-db`, and `coder` all reporting `healthy` (or `running`). The `coder` service runs a healthcheck, so give it a few seconds on first start.

> **Port 6379 conflict:** If the container fails with `failed to bind host port 0.0.0.0:6379/tcp: address already in use`, another process or container is already bound to port 6379 (e.g. a `streamr-redis` container). Remove or stop the conflicting container, or change the port mapping in `docker-compose.yml`.

---

## Step 3 — Bootstrap (one-time setup)

```bash
./scripts/prod.sh bootstrap
```

This will:

1. **Build and sync dev binaries** — compiles `openflows` (controller) and `openflows-harness` (worker coordination) in release mode and copies both to `.dev-binaries/` for mounting into Coder workspaces.
2. **Create the admin user in Coder** — creates the initial admin account (see Step 4 for credentials).
3. **Push workspace templates** — deploys the `nexus`, `forge`, `sentinel`, `vessel`, and `lore` templates via `coder templates push`.
4. **Verify LLM/GitHub auth** — ensures a GitHub token and at least one LLM model are configured.

---

## Step 4 — Sign in, add a Coder license & get the API token

The bootstrap script creates the initial Coder admin account. By default the credentials are:

| Field | Default |
|-------|---------|
| Username | `admin` |
| Email | `admin@openflows.dev` |
| Password | `Op3nFl0ws!` |

Override them with `CODER_ADMIN_USERNAME` / `CODER_ADMIN_EMAIL` / `CODER_ADMIN_PASSWORD` before running bootstrap.

> **Password requirements:** If the `CODER_ADMIN_PASSWORD` you set does not meet Coder's security requirements (at least 8 characters, and containing an uppercase letter, a lowercase letter, a digit, and a special character), bootstrap **silently falls back to `Op3nFl0ws!`** and creates the admin with that instead. Either set a password that satisfies these requirements, or sign in with the default `Op3nFl0ws!` (check the bootstrap output for the "falling back to default" warning).

Then:

1. Open **http://localhost:7080**
2. **Sign in with the admin credentials above** (first-time only). The bundled Coder service authenticates with a username/password — it does **not** come with GitHub OAuth pre-configured. GitHub sign-in is only available if you manually configure a GitHub OAuth provider in Coder afterwards; it is not required to get started.
3. **Add a Coder license** — create a license from your account at coder.com, then add it at **http://localhost:7080/deployment/licenses/add**. (Coder requires a valid license before some functionality is enabled. For local development you can use Coder's free/developer license — see <https://coder.com/docs/next/admin/licenses>.)
4. Click your **username** (top-right corner) → **Account** → **Tokens**.
5. Click **Create Token**, copy the token, and paste it into `.env` as:
   ```bash
   CODER_SESSION_TOKEN=your_token_here
   ```

---

## Step 5 — Add a tenant

```bash
./scripts/prod.sh tenant <owner/repo> --name <my-team>
```

This binds a GitHub repo to the controller. You must add at least one tenant before starting the controller. See [TOKEN_GUIDE.md](TOKEN_GUIDE.md) for the token acquisition walkthrough.

---

## Step 6 — Run the controller

```bash
# Run this in a separate terminal; the controller remains in the foreground
./scripts/prod.sh run
```

This **always** resets Redis to a clean slate, then starts the controller in the foreground (logs stream to this terminal). Create a GitHub issue in a bound repo → OpenFlows automatically assigns, provisions a workspace, and starts working.

---

## Step 7 — Verify it's working

Because the controller runs in the foreground, its logs stream directly to the terminal where you started `./scripts/prod.sh run`. In a **separate terminal** you can verify:

```bash
# Health check
./scripts/prod.sh doctor
```

Confirm the Docker services are healthy:

```bash
docker compose ps
```

A successful health check shows Coder and Redis healthy. Verify the controller separately by confirming that its foreground terminal remains running and streams sync/provisioning activity after you create an issue.

> **On `/tmp/openflows-controller.log`:** That log file only exists in the **production** flow, where the controller runs inside a Nexus workspace and its startup script redirects output (`openflows run >/tmp/openflows-controller.log`). Locally, `prod.sh run` runs in the foreground — watch that terminal instead.

---

## Troubleshooting

### `Failed to run coder templates push` (during bootstrap)

The `coder` CLI is missing or not on your `PATH`. Bootstrap shells out to `coder templates push`. Install it:

```bash
curl -fsSL https://coder.com/install.sh | sh
```

Ensure `coder` is on your `PATH`, confirm with `coder version`, then re-run bootstrap.

### "No LLM models configured in Coder"

Open the Coder dashboard → **AI Settings** → **Coder Agents** → **Models** and configure at least one provider/model, then re-run bootstrap.

### `cp: cannot create regular file '.dev-binaries/openflows': Permission denied`

The `.dev-binaries/` directory is `root`-owned. Take ownership:

```bash
sudo chown -R "$USER":"$USER" .dev-binaries/
```

### Missing required environment variables

Make sure `.env` is in the project root:

```bash
cp .env.example .env
# Edit .env with your tokens
```

### Redis container not responding

```bash
docker ps | grep redis
docker compose up -d   # restart if needed
```

### `failed to bind host port 0.0.0.0:6379/tcp: address already in use`

Another process or container already holds port 6379. Find the conflicting container with `docker ps --format '{{.Names}}\t{{.Ports}}' | grep 6379` and remove/stop it (e.g. `docker rm -f streamr-redis`), or change the redis port mapping in `docker-compose.yml`.

### Controller not picking up issues

1. Confirm a tenant is bound (`./scripts/prod.sh tenant <owner/repo> --name <my-team>`).
2. Check the terminal running the controller for errors (locally) — or `tail -f /tmp/openflows-controller.log` in the production flow.
3. Verify Coder is reachable: `curl http://localhost:7080/api/v2/buildinfo`.

---

## For More Details

- **Full Documentation**: See [README.md](README.md)
- **Testing & Debugging**: See [TESTING_QUICK_START.md](TESTING_QUICK_START.md)
- **Token Acquisition**: See [TOKEN_GUIDE.md](TOKEN_GUIDE.md)
