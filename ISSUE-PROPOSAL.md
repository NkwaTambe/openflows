# README: Complete setup steps from docker compose to running the controller are incomplete

The README.md setup section currently lacks complete, beginner-friendly instructions for getting from zero to a running controller. New users face gaps that require hunting through multiple documents.

## Current Issues

The current Quick Start section reads:
```bash
cp .env.example .env
# Edit .env: set GITHUB_TOKEN but leave CODER_SESSION_TOKEN empty
./scripts/prod.sh bootstrap
./scripts/prod.sh tenant owner/repo --name my-team
./scripts/prod.sh run
```

This assumes too much knowledge:

### 1. Docker Compose is never mentioned
New users don't know they need to start `docker compose` first. The critical step to start Redis, Coder DB, and Coder itself is completely missing:
```bash
docker compose up -d
```

### 2. Token acquisition process is unclear
The README says to "set GITHUB_TOKEN but leave CODER_SESSION_TOKEN empty" but never explains:
- Where to get GITHUB_TOKEN (github.com/settings/tokens)
- How to get CODER_SESSION_TOKEN (only available AFTER Coder is running and you've created an admin account)
- What GITHUB_REPOSITORY should be

### 3. The bootstrap command throws errors without prerequisites
- `./scripts/prod.sh bootstrap` requires Rust 1.70+ to build the binaries
- If you're on a new machine, the build will fail without mentioning what's missing

### 4. No verification steps
After running all commands, there's no guidance on:
- How to verify Redis/Coder are healthy
- How to check if the controller started successfully
- What logs to watch

## Proposed Solution

Add a complete, linear setup section to README.md:

```markdown
## Complete Setup (from scratch)

### 1. Start Docker infrastructure
```bash
docker compose up -d
```
This starts Redis (port 6379), PostgreSQL for Coder, and Coder itself (port 7080).

Wait for services to be healthy:
```bash
docker compose ps
```

### 2. Configure environment
```bash
cp .env.example .env
```

Edit `.env` with your values:
- `GITHUB_TOKEN`: From https://github.com/settings/tokens (needs `repo` scope)
- `GITHUB_REPOSITORY`: Your repo in format `owner/repo`
- `CODER_SESSION_TOKEN`: **Leave empty for now** (you'll add this after bootstrap)

### 3. Bootstrap (one-time setup)
```bash
./scripts/prod.sh bootstrap
```
This:
- Builds `openflows` and `openflows-harness` binaries
- Creates your admin account in Coder
- Pushes workspace templates
- Verifies API access

### 4. Get Coder session token
1. Open http://localhost:7080
2. Create your admin account (first-time only)
3. Sign in → Click username → Account → Tokens → Create Token
4. Copy token to `.env` as `CODER_SESSION_TOKEN=cdr_...`

### 5. Add a tenant
```bash
./scripts/prod.sh tenant owner/repo --name my-team
```

### 6. Run the controller
```bash
./scripts/prod.sh run
```

### 7. Verify it's working
```bash
# Check controller log
tail -f /tmp/openflows-controller.log

# Health check
./scripts/prod.sh doctor
```

## Additional Improvements Needed
- Add a Prerequisites section that lists Rust 1.70+ (for bootstrap), Node 18+ (for GitHub MCP), Docker 24+
- Consider adding a "Troubleshooting" section for common bootstrap failures
- Reference TOKEN_GUIDE.md more prominently in the Quick Start

## References
- docker-compose.yml: defines Redis, coder-db, and coder services
- QUICK_START.md: has more detail but is also missing the `docker compose up` step
- TOKEN_GUIDE.md: helpful but buried in the docs