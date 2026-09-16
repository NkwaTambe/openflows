# Scripts

## Production Commands

All production operations use `./scripts/prod.sh`:

```bash
./scripts/prod.sh bootstrap                      # One-time: Setup Coder + push templates
./scripts/prod.sh tenant owner/repo --name team  # Add a tenant (required before run)
./scripts/prod.sh run                            # Start controller (always resets Redis first)
./scripts/prod.sh doctor                         # Health check
```

### `bootstrap` — One-time Setup

```bash
./scripts/prod.sh bootstrap
```

- Creates admin user in Coder (if not exists)
- Pushes Coder templates (nexus, forge, etc.)
- Verifies LLM and GitHub external auth are configured

### `tenant` — Add a Team

```bash
./scripts/prod.sh tenant owner/repo --name my-team
```

Binds a GitHub repo to the controller. **You must add at least one tenant before starting the controller.**

### `run` — Start Controller

```bash
./scripts/prod.sh run
```

**Always resets Redis to a clean slate first**, then starts the controller. This ensures no zombie tickets or stale state from previous runs. OpenFlows will process issues in bound repos.

### `doctor` — Health Check

```bash
./scripts/prod.sh doctor
```

---

## Development Helpers

### `dev-sync.sh` — Build and Mount Dev Binary

Builds the OpenFlows controller and makes it available to Coder workspaces:

```bash
./scripts/dev-sync.sh
```

This:
1. Builds the `openflows` release binary (if not already built)
2. Copies it to `.dev-binaries/` for Docker mounting
3. Hot-deploys into any running Coder workspace (optional)

**Note:** `./scripts/prod.sh bootstrap` runs this automatically. Use manually if you rebuild the binary and want to hot-deploy into a running workspace.

### `reset-controller-state.sh` — Clean Redis

Reset Redis to a clean state:

```bash
./scripts/reset-controller-state.sh --confirm
```

Removes all tickets, workers, and orchestration state.

### `install.sh` — CLI Installer

Installs the `openflows` CLI binary:

```bash
curl -fsSL https://get.openflows.dev | bash
```

---

## Production Architecture

The controller runs inside a **Nexus workspace** provisioned by Coder. The workspace auto-starts the controller via startup_script.

```
openflows bootstrap → Coder pushes templates
openflows tenant add → Coder creates nexus workspace
    ↓
Workspace starts (docker container)
    ↓
Startup script runs:
  → Installs openflows binary
  → Sets up orchestration volume
  → Starts heartbeat daemon
  → Executes: openflows run
    ↓
Controller starts inside workspace
```

---

## Quick Reference

| Command | Description |
|---------|-------------|
| `./scripts/dev-sync.sh` | Build and mount dev binary to Coder |
| `./scripts/prod.sh bootstrap` | One-time: Setup Coder + templates (includes dev-sync) |
| `./scripts/prod.sh tenant owner/repo --name team` | Add tenant (required before run) |
| `./scripts/prod.sh run` | Clean slate + start controller |
| `./scripts/prod.sh doctor` | Health check |
| `./scripts/reset-controller-state.sh --confirm` | Clean Redis state |