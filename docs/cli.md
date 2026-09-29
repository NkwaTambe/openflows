# OpenFlows remote CLI (`openflows-cli`)

The remote CLI lets an operator command a **deployed** OpenFlows instance from
outside its host network — manage tenants, assign work directly, and halt or
resume a fleet — without direct access to Redis or Coder. It talks to the
`openflows-manager` HTTP control surface over bearer-token auth.

This is the client side of the control surface described in
[issue #333](https://github.com/The-AgenticFlow/openflows/issues/333).

## Install

Build from the workspace:

```sh
cargo build -p openflows-cli
# binary: target/debug/openflows-cli
```

## Configure / login

The CLI reads its manager URL and token from a local config file
(`~/.config/openflows/config.toml`) or from flags/environment. Save them once:

```sh
openflows-cli login --url https://openflows.example.com --token <TOKEN>
```

The token can also come from the `OPENFLOWS_MANAGER_TOKEN` environment variable
or the `--token` flag, and the URL from `OPENFLOWS_MANAGER_URL` / `--url`.
Credentials stored in the config file are written owner-only (`0600`).

> The token must match the manager's `OPENFLOWS_MANAGER_TOKEN` (set when the
> manager starts). Requests without a valid token are rejected with a clear
> `401`.

## Usage

All commands support `--json` for machine-readable output. On success the CLI
exits `0`; on any error it exits non-zero and prints the error to stderr.

### Tenants

```sh
# List tenants
openflows-cli tenant list
openflows-cli tenant list --json

# Add a tenant bound to a GitHub repo (fleet = FORGE-SENTINEL pairs)
openflows-cli tenant add my-org/my-repo --name my-team --fleet 3
```

### Control (halt / resume)

```sh
# Read a tenant's control mode (auto | paused | drained | targeted)
openflows-cli control get my-team

# Halt the fleet (controller skips work on its next poll)
openflows-cli control pause my-team

# Resume (back to auto)
openflows-cli control resume my-team

# Set any documented mode explicitly
openflows-cli control set my-team drained
```

### Assigning work

A task can be supplied three ways — as flags, as inline JSON, or from a JSON
file:

```sh
# Flags
openflows-cli tasks assign my-team --title "Fix the bug" --body "details"

# Inline JSON
openflows-cli tasks assign my-team \
  --json-input '{"title":"Fix the bug","payload":{"kind":"bug"}}'

# JSON file
openflows-cli tasks assign my-team --file task.json
```

A task JSON object may contain `title`, `body`, and an arbitrary `payload`.
Only `title` is required.

## Examples

```sh
# Add a tenant and immediately pause it for maintenance
openflows-cli tenant add my-org/my-repo --name my-team --fleet 3
openflows-cli control pause my-team

# ...later, resume and hand the team a task
openflows-cli control resume my-team
openflows-cli tasks assign my-team --title "Ship the release notes"
```

## Behavior notes

- **Pause takes effect on the next controller poll.** The manager writes the
  control mode to the tenant's shared store; the tenant's controller reads it
  at the top of each 15s poll and skips work while `paused`.
- **Secrets are never printed.** The token is only sent in the `Authorization`
  header; error output does not echo credentials.
- **Deterministic exit codes**: `0` success, non-zero on failure (auth,
  network, validation).
