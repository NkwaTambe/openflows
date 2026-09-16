# Changelog

All notable changes to this project will be documented in this file.
<!-- markdownlint-disable line-length no-bare-urls ul-style emphasis-style -->

## [1.3.0] - 2026-09-16

### Features

- [2cae85c](
https://github.com/NkwaTambe/openflows/commit/2cae85c26342001fefc72f4585acbe19fa3d660e) *(coder)* Pin Coder to v2.37.0 and validate version in doctor ([#186](https://github.com/The-AgenticFlow/openflows/pull/186))

  > Replace the :latest image tag default with v2.37.0 across docker-compose,
  > config defaults, .env.example and docs, and validate the running Coder
  > reports the pinned version via /api/v2/buildinfo in doctor.

- [a8b4f8c](
https://github.com/NkwaTambe/openflows/commit/a8b4f8cf59fd0ea170cd6aceeea7ad1135c2afeb) *(config)* Centralize remaining env reads incl GITHUB_API_BASE (#185, #204)

  > Finish the crate-level migration so all environment-variable reads route
  > through the centralized config::env structs instead of inline env::var calls:
  >
  > - Add GITHUB_API_BASE to GithubConfig (default https://api.github.com) and
  >   resolve it in GithubRestClient via GithubConfig::init_from_env.
  > - Migrate inline reads in coder-client, agent-nexus, agent-vessel,
  >   agent-sentinel, agent-forge, agent-lore, openflows-harness, pocketflow-core,
  >   and the openflows binary (agentflow/debug/doctor/orchestration).
  > - Re-export config::Envconfig so consumers can call XxxConfig::init_from_env.
  > - Add config test for GITHUB_API_BASE default/override; update env audit.
  >
  > Documented exclusions (CODER_API_TOKEN, CODER_TRANSPORT_VERBOSE,
  > CODER_WORKSPACE_ID, notifier vars, ARTIFACTS_DIR, TF_VAR_*, HOME/USERPROFILE)
  > and dynamic keys stay inline per docs/env-config-audit.md; OPENFLOWS_TAR in
  > build.rs cannot use the central config crate (build scripts only see
  > build-dependencies).

- [266d517](
https://github.com/NkwaTambe/openflows/commit/266d51781c144f57bcc214f1a654a74705ade709) *(config)* Centralize environment configuration with envconfig ([#185](https://github.com/The-AgenticFlow/openflows/pull/185))

  > Introduce a centralized, type-safe config layer in crates/config using the
  > envconfig crate. EnvConfig aggregates Coder/Infra/Tenant/Github/Agent configs
  > with envconfig defaults and clear startup validation.
  >
  > - Add envconfig 0.11 to the workspace and config crate.
  > - New crates/config/src/env.rs with EnvConfig and per-domain structs.
  > - Wire the openflows controller (binary) and openflows-harness startup to load
  >   config centrally instead of inline std::env::var reads.
  > - Add env_config_test.rs covering defaults, overrides, parse failures and
  >   controller validation.
  > - Add docs/env-config-audit.md inventory classifying every env var.
  > - Update .env.example to the centralized variable set.
  >
  > Excluded from the centralized layer: notifier (SLACK/DISCORD/WHATSAPP) config,
  > CODER_API_TOKEN (duplicate of session token), CODER_ADMIN_USERNAME (email
  > login), CODER_TRANSPORT_VERBOSE and CODER_WORKSPACE_ID.

- [ad7668a](
https://github.com/NkwaTambe/openflows/commit/ad7668acccaf4aad2bce63f40b31a0a177fb9932) *(lore)* Commit and push docs PR after generating documentation by @Christiantyemele in [#61](
https://github.com/NkwaTambe/openflows/pull/61)

  > - Added commit_and_push_docs() to create lore/docs-* branch and push changes
  > - Added open_docs_pr() to create PR via GitHub MCP and add to pending_prs
  > - Updated post() to automatically commit/push/open PR for all doc changes
  > - Exported McpGithubClient from github crate for reuse
  > - Docs now flow through VESSEL merge pipeline like code PRs

- [23c9cc5](
https://github.com/NkwaTambe/openflows/commit/23c9cc5a2c8c760fb3bee77fbd112d83eb5a9eaf) *(nexus)* Add real E2E test, mcp-proxy bridge, and enhanced logging by @Christiantyemele

- [f6033f0](
https://github.com/NkwaTambe/openflows/commit/f6033f037ab32f359e96e2881717e1921563633a) *(phase2)* Implemented nexus by @Christiantyemele

- [41350ae](
https://github.com/NkwaTambe/openflows/commit/41350ae924d654eed091ccc5a961674fb051e9f5) *(uncategorized)* Prefer GitHub PAT for workspace git, fix repo acquisition

  > Prefer an explicit GitHub Personal Access Token (github_pat) for git
  > clone/push in Coder worker workspaces, falling back to the Coder GitHub App
  > external-auth token. A repo-scoped PAT works regardless of GitHub App
  > install scope (which is user-account-only and cannot be installed on
  > orgs/repos the app isn't authorized for).
  >
  > - Add github_pat workspace parameter to forge/sentinel/vessel/lore templates
  >   and the nexus creds block; controller passes the PAT when provisioning.
  > - Acquire the repo via the per-tenant golden seed (copy -> refresh ->
  >   checkout) with sudo, falling back to a sudo clone; empty workspaces now
  >   log a visible warning instead of failing silently.
  > - Bump host Coder CLI to match server (v2.37.1) so provisioner writes work.
  > - Coder-only env/docs/tools renames and lifecycle-hook config updates.

- [a54093d](
https://github.com/NkwaTambe/openflows/commit/a54093d818d5e58519d6935d8fef0180d75bddd8) *(uncategorized)* Split releases into openflows and openflows-harness without v prefix

  > Release two independent GitHub releases (openflows-X.Y.Z and
  > openflows-harness-X.Y.Z, no 'v' prefix) instead of one combined vX.Y.Z
  > release. Each binary gets its own tag, release, and cross-platform
  > assets.
  >
  > - release-plz.toml: drop version_group, add per-package git_tag_name
  >   (openflows-{{version}} / openflows-harness-{{version}})
  > - release-assets.yml: trigger on per-package tags and attach only that
  >   package's binaries to its own release
  > - scripts/install.sh: download both releases (openflows + harness) at the
  >   resolved version; resolve openflows-* tags directly
  > - Cargo.toml: add missing version fields to internal path dependencies so
  >   release-plz can cargo package (unblocks release-pr)
  > - RELEASING.md: document the two-release workflow

- [7a75b43](
https://github.com/NkwaTambe/openflows/commit/7a75b434f5217dc57eb3381af887136f9a41afc0) *(uncategorized)* Adopt release-plz with develop-as-default branching

  > - Add develop as integration default; main becomes release-only
  > - Add release-plz.toml (git_only, no crates.io) with lock-step openflows +
  >   openflows-harness under a single vX.Y.Z version_group tag
  > - Replace release.yml with release-plz release job (push to main)
  > - Add release-assets.yml: build cross-platform tarballs on the v* tag push
  >   and attach to the release (predictable/fixed asset names, overwrite)
  > - Add release-branch.yml guard: only release/vX.Y.Z PRs may target main
  > - Remove harness-publish.yml (edge channel folded into release-plz)
  > - Repoint ci.yml + semver_checks.yml to develop
  > - Add RELEASING.md
  > - Bump binary + harness to 1.2.0 (lock-step)

- [b0a06fc](
https://github.com/NkwaTambe/openflows/commit/b0a06fce0eb661db23482fd5e38d36cc618494d5) *(uncategorized)* Add tenant clean command and gate approval system

  > - Add 'tenant clean' command to reset stale tickets (awaiting_human/failed) back to Open
  > - Add --reset-all flag to reset ALL tickets regardless of status
  > - Clear worker_slots to prevent stale workspace IDs from blocking provisioning
  > - Clear recovery attempt counters during cleanup
  > - Add gate approval system for phase transitions
  > - Add Sentinel spawn routing in flow controller
  > - Add review_ready action for Forge → Sentinel handoff

- [1f6d2bf](
https://github.com/NkwaTambe/openflows/commit/1f6d2bfacd8c7802c9f7ca486f6340f38098b77a) *(uncategorized)* Implement Coder integration plan (Phases 1-3)

  > Complete implementation of the Coder OSS integration, enabling OpenFlows
  > to run FORGE-SENTINEL pairs in isolated Coder workspaces alongside local
  > git worktree mode.
  >
  > Phase 1 — WorkspaceTransport abstraction:
  > - Add WorkspaceTransport trait with read_file, write_file, execute, list_directory
  > - Implement LocalTransport (wraps tokio::fs and tokio::process::Command)
  > - Parameterize provision.rs to accept &dyn WorkspaceTransport
  > - Add WorkspaceProvider enum (Local/Coder) to WorkerSlot and config
  >
  > Phase 2 — SharedStore migration for pair state:
  > - Add pair_keys module (8 key functions + 4 extras)
  > - Add PairStateStore trait, PairArtifact enum, FilesystemPairState, SharedStorePairState
  > - Migrate provision.rs write_ticket/write_task to use PairStateStore
  > - Add PairStateWatcher trait, FilesystemWatcher, SharedStoreWatcher
  > - Add PairWatcher enum (Local/Coder) with WatcherAdapter and CoderWatcherAdapter
  > - Migrate pair.rs event loop from SharedDirWatcher to PairWatcher
  > - Add pair_state field to ForgeSentinelPair for dual-write support
  > - Migrate write_error_feedback/clear_error_feedback to use PairStateStore
  >
  > Phase 3 — Coder bundling + bootstrapper + transport:
  > - Add coder-client crate with full Coder REST API client
  > - Add CoderBootstrapper with from_env() and bootstrap()
  > - Add Terraform workspace templates (openflows-forge, openflows-sentinel)
  > - Implement CoderTransport behind #[cfg(feature = "coder")]
  > - Add CoderProvisioner logic in agent-nexus
  > - Add Coder workspace provisioning in agent-nexus
  > - Wire VESSEL to stop Coder workspaces on merge (Merged, CiMissing, recycle)
  > - Add Coder bootstrap sequence in main.rs before flow loop
  > - Add docker-compose.yml entries for coder + postgres services
  >
  > Phase 3.11 — TUI Coder integration:
  > - Add step_coder setup wizard (moved to step 4 as primary architecture choice)
  > - Add workspace_provider to RegistryEntry with From impl
  > - Dashboard: worker table shows Coder workspace column, header shows mode
  > - Doctor: Coder server reachable, templates pushed, Docker socket checks
  > - SETUP/ENV: WORKSPACE_PROVIDER env var now also checked at runtime
  > - Better error messages when Coder bootstrap fails
  >
  > Bug fixes and improvements:
  > - Fix write_task_context to use self.pair_state instead of local FilesystemPairState
  > - Fix clear_error_feedback to only write to PairStateStore when content exists
  > - Extract stop_coder_workspace_for_pr helper to deduplicate VESSEL merge handlers
  > - Fix pre-existing Result<CommandOutput> handling bugs in CoderTransport methods
  > - Add WORKSPACE_PROVIDER=coder fallback check when CODER_URL is missing
  > - Remove unused CODER_WORKSPACE_DIR constant

- [bdc2c2e](
https://github.com/NkwaTambe/openflows/commit/bdc2c2e2431cfbb1ef72fc4ee5b9003b7fe065ec) *(uncategorized)* Embed all orchestration files at compile time with self-healing and version tracking

  > - OrchestrationResolver embeds all 80 files (personas, standards, plugins,
  >   hooks, skills, commands, MCP templates) via include_str!() in bundled.rs
  > - ensure_orchestration_dir() writes missing files from embedded content on startup
  > - .version file tracks bundled version (CARGO_PKG_VERSION); warns if on-disk
  >   is stale, suggests running openflows --reset-orchestration
  > - reset_orchestration_dir() overwrites ALL bundled files with embedded defaults
  >   (explicit opt-in via --reset-orchestration CLI flag)
  > - validate() checks required personas exist with clear error diagnostics
  > - persona_path() resolves from on-disk orchestrator dir, with fallback search
  > - Removed copy_persona_files() from setup wizard (now redundant)
  > - Removed fragile resolve_persona_path() fallback in main.rs
  > - Both openflows and agentflow binaries use OrchestrationResolver via lib crate
  > - lib.rs added to expose orchestration module to both binaries
  > - install.sh build_from_source() now copies orchestration/ directory
  > - agent-nexus load_persona() has better error message suggesting missing .md files
  > - ORCHESTRATOR_DIR env var set for pair-harness subprocesses

- [f710aee](
https://github.com/NkwaTambe/openflows/commit/f710aee7aebcec78add153fc216ece6a55845854) *(uncategorized)* Add detailed prerequisites to README, refactor output indexing in responses proxy, extract strip_provider_prefix to agent-client

  > - Expand README 'What You Need' section into detailed Prerequisites with
  >   system requirements, AI backend options, optional services, and env setup
  > - Fix output_index ordering in responses_proxy: function calls get indices
  >   0..N-1, text message gets index N, ensuring ascending output_index sequence
  > - Extract strip_provider_prefix from pair-harness into agent-client crate
  > - Add agent-client dependency to pair-harness Cargo.toml
  > - Update Cargo.lock accordingly
  > - Minor formatting/clippy fixes across several crates

- [30ca4e8](
https://github.com/NkwaTambe/openflows/commit/30ca4e802ce4c8f73c7e01a9480726b3acafcc18) *(uncategorized)* Per-agent GitHub tokens with instance suffix support

  > Add registry-based token resolution that handles instance IDs (e.g., 'forge-1' -> 'forge'), configure remote URLs with embedded tokens for push auth, and improve docs PR detection to skip unnecessary LORE processing.

- [37d89ed](
https://github.com/NkwaTambe/openflows/commit/37d89edde92559ebaba884d1f9e27328b72404f5) *(uncategorized)* Per-agent GitHub tokens, git identity from PAT, and REST client extensions

  > - Add identity module and AgentIdentity/IdentityManager to config crate
  > - Add github_token_env field to RegistryEntry and resolve_github_token()
  >   helper that falls back to GITHUB_PERSONAL_ACCESS_TOKEN
  > - Add create_pull_request, close_pull_request, post_json, and branch-based
  >   ticket_id extraction to GithubRestClient
  > - Configure git user.name/email from PAT identity when creating worktrees
  >   so commits show the PAT owner instead of local git config
  > - Make create_worktree async and accept github_token parameter
  > - Add VesselConfig::from_registry and LoreConfig new_with_registry
  > - Update registry.json with per-agent github_token_env fields and
  >   glm-5 model backend
  > - Update binary main.rs and bin files to use registry-resolved tokens
  > - Add DocsPrClosed variant to VesselOutcome
  > - Update agent-client mcp/runner for token passthrough
  > - Minor test and demo fixes for new signatures

- [3ca3294](
https://github.com/NkwaTambe/openflows/commit/3ca32943e4345eb83fe1a97ca47e93f98ec249f6) *(uncategorized)* Add conflict resolver, enhance vessel/nexus agents, and improve proxy-first routing by @Christiantyemele in [#31](
https://github.com/NkwaTambe/openflows/pull/31)

  > - Add conflict_resolver.rs for git conflict detection and classification
  > - Enhance vessel agent with CI gate improvements and notifier updates
  > - Expand nexus agent with orchestration and triage capabilities
  > - Refactor agent-client fallback with proxy-first key priority
  > - Update pair-harness with isolation, process, and workspace improvements
  > - Add system-behavior architecture documentation
  > - Clarify .env.example for proxy/gateway/direct mode priority
  > - Remove .windsurfrules (emptied), update CONTRIBUTING.md
  > - Update agent registry and skill definitions

- [001dad9](
https://github.com/NkwaTambe/openflows/commit/001dad96401be30bebdca25bd1324e190a0b6125) *(uncategorized)* Implement VESSEL agent (DevOps & Merge Gate) by @Christiantyemele in [#31](
https://github.com/NkwaTambe/openflows/pull/31)

  > Implements issue #2 - VESSEL agent for CI polling, PR merging, and
  > dependency resolution.
  >
  > ## Changes
  >
  > ### New Crate: agent-vessel
  > - ci_poller.rs: CI status polling with configurable timeout
  > - merger.rs: PR merge execution with squash strategy
  > - notifier.rs: Event emission for dependency resolution
  > - node.rs: VesselNode implementing Node trait
  > - types.rs: VesselConfig, VesselOutcome types
  > - 19 unit tests covering all modules
  >
  > ### Shared Types (pocketflow-core)
  > - CiStatus, CiPollConfig, MergeMethod, MergeResult, PrInfo, PrState
  > - 8 new tests for type serialization
  >
  > ### GitHub REST Client
  > - rest.rs: Direct GitHub API for CI status and merge operations
  > - get_ci_status(), get_pull_request(), merge_pull_request()
  >
  > ### Config Updates
  > - TicketStatus::Merged variant for dependency resolution
  > - KEY_PENDING_PRS for PR queue
  > - ACTION_DEPLOYED, ACTION_DEPLOY_FAILED constants
  >
  > ### Flow Integration
  > - ForgePairNode adds opened PRs to pending_prs
  > - VESSEL routes: pr_opened -> vessel -> nexus
  > - Full flow: nexus -> forge_pair -> vessel -> nexus
  >
  > ### Documentation
  > - docs/architecture/vessel-agent.md with diagrams
  >
  > ## Acceptance Criteria (from #2)
  > - [x] VesselNode implements Node trait
  > - [x] CI polling until terminal state
  > - [x] Timeout handling for CI hangs
  > - [x] Merge execution via GitHub API
  > - [x] ticket_merged event emission
  > - [x] Failure isolation (CI failure = no merge)

- [4b0f1db](
https://github.com/NkwaTambe/openflows/commit/4b0f1db95406f2949406d86837b234f0e0b485f9) *(uncategorized)* Complete FORGE-SENTINEL pair lifecycle with SENTINEL review certification by @Christiantyemele in [#12](
https://github.com/NkwaTambe/openflows/pull/12)

  > Major improvements to the autonomous development workflow:
  >
  > ## FORGE Agent Improvements
  > - Use --dangerously-skip-permissions for immediate tool execution
  > - Simplified settings.json (permissions handled by CLI flag)
  > - Clear prompts distinguishing worktree (source code) vs shared (artifacts)
  > - Segment-by-segment implementation with SENTINEL review gates
  > - PR creation only after SENTINEL final approval
  >
  > ## SENTINEL Review Integration
  > - Plan review: SENTINEL evaluates PLAN.md, writes CONTRACT.md
  > - Segment evaluation: SENTINEL reviews each segment, writes segment-N-eval.md
  > - Final review: SENTINEL certifies code quality, writes final-review.md
  > - All segments must be APPROVED before PR creation
  > - Final review includes SENTINEL signature and certification
  >
  > ## LLM Provider Fallback Support
  > - Added FallbackClient for automatic provider failover
  > - Added GeminiClient for Google Gemini API support
  > - Pass all LLM env vars to spawned processes
  > - Configurable via LLM_PROVIDER and LLM_FALLBACK env vars
  >
  > ## Workflow State Tracking
  > - Added plan_approved and final_approved flags
  > - Added all_segments_approved() to track segment progress
  > - FORGE respawned with correct prompt after each phase
  > - spawn_forge_for_pr() for dedicated PR creation mode
  >
  > ## Nexus Agent Improvements
  > - Pre-parse repository into owner/repo_name in context
  > - Updated prompt to use list_issues directly with owner/repo
  > - Explicit instructions to avoid search_repositories confusion
  >
  > ## Bug Fixes
  > - Fixed PLAN.md writing loop after contract agreed
  > - Fixed wrong directory for source code (worktree vs shared)
  > - Fixed missing LLM env vars in spawned processes
  > - Fixed API key override from ~/.bashrc

- [ceb4aef](
https://github.com/NkwaTambe/openflows/commit/ceb4aefdac4afe69188b4722337bc7442ce7fd4a) *(uncategorized)* Switch real_test to use ForgePairNode for full SENTINEL lifecycle by @Christiantyemele in [#12](
https://github.com/NkwaTambe/openflows/pull/12)

  > - Replace ForgeNode with ForgePairNode in real_test.rs
  > - Enables event-driven FORGE-SENTINEL pair architecture
  > - Creates shared/ directory with PLAN.md, WORKLOG.md, CONTRACT.md
  > - Enables SENTINEL evaluations: segment-N-eval.md, final-review.md
  > - STATUS.json now written in shared/ directory as per architecture
  > - Follows forge-pair-integration.md implementation plan

- [1ea4c81](
https://github.com/NkwaTambe/openflows/commit/1ea4c8152d08068bd61254c768fa194f9fa248c9) *(uncategorized)* Complete Phase 4 orchestration with robust JSON extraction and worker logging by @Christiantyemele

- [6651837](
https://github.com/NkwaTambe/openflows/commit/6651837822975e49735f14a9ce46fafb6f661fd7) *(uncategorized)* Implement hosted MCP bridge, nexus E2E tests, and contributing guide by @Christiantyemele

### Bug Fixes

- [e790d1c](
https://github.com/NkwaTambe/openflows/commit/e790d1c8ee602c7c0b151f75cf3d0fcd05cc4050) *(ci)* Resolve clippy warnings and unused dependency in hook consumer

  > - kick_bus: detach fred manage_subscriptions via mem::drop instead of
  >   non-binding let on a future (clippy::let-underscore-future)
  > - agentflow: drop identity map on build_kick_bus result (clippy::map-identity)
  > - hooks: collapse sentinel gate if, and tidy doc-list formatting
  > - agent-nexus: remove unused base64 dependency
  >
  > Clippy (--workspace --all-targets --all-features -D warnings), cargo-machete,
  > and fmt all pass locally.

- [5fe1d6f](
https://github.com/NkwaTambe/openflows/commit/5fe1d6fd643bfc8409f4c3a6636e3312ecab36a3) *(ci)* Resolve failing checks (fmt, clippy, dead-code, unused dep, typo)

- [49d0993](
https://github.com/NkwaTambe/openflows/commit/49d099365e166431f4913e9963ef211a818b8f46) *(ci)* Resolve 4 failing CI checks — spelling, clippy, format, cargo deny by @Christiantyemele in [#40](
https://github.com/NkwaTambe/openflows/pull/40)

  > - Spelling: fix 'mis-mapping' typo in pair.rs
  > - Clippy: simplify nonminimal_bool in pair.rs:1968
  > - Format: run cargo fmt across all modified crates
  > - Cargo deny: update rustls-webpki 0.103.12→0.103.13 (RUSTSEC-2026-0104)
  >   and remove stale RUSTSEC-2026-0097 advisory ignore

- [2685534](
https://github.com/NkwaTambe/openflows/commit/2685534aa8ed07425babb687d2fa73a06b084c25) *(ci)* Resolve forge CI fix loop — watchdog kills stalled pairs, structured failure detail, worklog enforcement by @Christiantyemele in [#40](
https://github.com/NkwaTambe/openflows/pull/40)

  > Three root causes fixed:
  >
  > 1. Wrong CI failure reason: get_failed_checks_detail() only fetched check-run
  >    annotations (mostly Node.js deprecation noise), not actual job logs. Added
  >    get_failed_job_logs() using Actions API (/actions/runs → /actions/jobs → logs)
  >    and CiFailureDetail struct with local_reproduce_commands() that map check
  >    names to exact install+run commands including apt-get/pip/venv setup.
  >
  > 2. .claude/ appended to .gitignore on every commit: ensure_exclusions() compared
  >    l.trim() (.claude/) against entry.trim_end_matches('/') (.claude) — never
  >    matched, so .claude/ was added every push cycle. Fixed to check both variants.
  >
  > 3. Watchdog only warned on stall, never killed: pairs ran 10000+ seconds doing
  >    nothing. Now kills forge process and returns PairOutcome::Blocked.
  >
  > CI_FIX.md and forge task prompt now emphasize: install missing tools (don't
  > skip checks), update WORKLOG.md as you work (watchdog monitors it), fix ALL
  > errors before pushing once.

- [7bcd407](
https://github.com/NkwaTambe/openflows/commit/7bcd40707eb8fd2ec2a95952fc0bad149b812110) *(coder)* Use Coder external-auth token for workspace git access by @NkwaTambe in [#220](
https://github.com/NkwaTambe/openflows/pull/220)

  > Agents could not clone/push private repos even after the GitHub App was
  > configured. The workspace templates tried to fetch a git token via the
  > legacy gitauths API (wrong provider path, unreachable URL) and then
  > overrode git credentials with an empty token, so no valid credential
  > ever reached the workspace.
  >
  > - Declare data "coder_external_auth" (id=primary-github) in all 5
  >   templates, which surfaces the 'Login with GitHub' button and provides
  >   a real access token; stop shadowing Coder's git auth.
  > - Fix doctor env-var check: CODER_EXTERNAL_AUTH_0_SECRET ->
  >   CODER_EXTERNAL_AUTH_0_CLIENT_SECRET.

- [b3d0307](
https://github.com/NkwaTambe/openflows/commit/b3d03079b7c98e9ebc833b725a1c8153d678c4b9) *(config)* Address PR review feedback ([#204](https://github.com/The-AgenticFlow/openflows/pull/204))

  > Reliability fixes surfaced in review:
  >
  > - OPENFLOWS_TENANT is no longer silently defaulted for the controller and
  >   harness: TenantConfig.tenant is now Option<string> with an effective_tenant()
  >   fallback, and validate_controller() fails when it is not explicitly set.
  > - The harness again requires REDIS_URL (previously it fell back to the local
  >   default, hiding a broken template): InfraConfig.redis_url is Option<string>
  >   with effective_redis_url() for callers that accept the fallback.
  > - USE_AI_GATEWAY no longer blocks startup of unrelated processes: parsed
  >   leniently (accepts "true"/"1") like the existing registry parser.
  > - Align external-auth field names with .env.example/docker-compose
  >   (CODER_EXTERNAL_AUTH_0_CLIENT_ID / _CLIENT_SECRET).
  > - Redact secrets in Debug output for CoderConfig, TenantConfig and GithubConfig.
  > - Guard tests against leaking process-wide env mutations by snapshotting and
  >   restoring variables.

- [b92c90e](
https://github.com/NkwaTambe/openflows/commit/b92c90e4dfdb4bae41bbb6e805927cbd42a52062) *(doctor)* Require external-auth client ID; centralize via CoderConfig ([#219](https://github.com/The-AgenticFlow/openflows/pull/219)) by @NkwaTambe in [#220](
https://github.com/NkwaTambe/openflows/pull/220)

- [b8caa4b](
https://github.com/NkwaTambe/openflows/commit/b8caa4b97bf6cf7b01882560ab94c6eeaa499f91) *(doctor)* Only compare semantic-version Coder tags, skip floating tags

  > Addresses review: a floating CODER_IMAGE_TAG such as 'latest' was coerced to
  > the invalid 'vlatest' and always reported version drift. Only pad unprefixed
  > semantic versions (e.g. 2.37.0 -> v2.37.0) and run exact version-drift checks
  > for semver tags; floating tags are left untouched and reported as informational.

- [7d9d9cb](
https://github.com/NkwaTambe/openflows/commit/7d9d9cb7339f32b22bbf30413aa12e2314b150a7) *(forge)* Claude Code stdin prompt and workspace isolation by @Christiantyemele in [#12](
https://github.com/NkwaTambe/openflows/pull/12)

  > - Fix Claude Code invocation error by passing prompt via stdin instead of
  >   command-line argument when using --allowedTools flag
  > - Add persona_path parameter to ForgeNode, loading agent persona from
  >   .agent/agents/forge.agent.md (source of truth for all agents)
  > - Create WorkspaceManager to clone target GitHub repository into
  >   ~/.agentflow/workspaces/ for proper isolation
  > - Update binary entry points to use cloned workspace instead of
  >   orchestrator's own directory
  > - Update all test files to match new constructor signatures

- [7053f95](
https://github.com/NkwaTambe/openflows/commit/7053f959f3a8d688ef189687d2e821388238b2e2) *(hooks)* Harden lifecycle hooks per review feedback

- [04be0f2](
https://github.com/NkwaTambe/openflows/commit/04be0f2acc195fbe81b720b1c4bf71c6a1e2bc28) *(lifecycle-hooks)* Recover hook orchestration and replay pending events

  > Recover hook-driven orchestration after interruptions so the control plane
  > resumes pending lifecycle events instead of leaving a stalled agent.

- [7fa9f66](
https://github.com/NkwaTambe/openflows/commit/7fa9f667bdf95befb2d97f7e9b94b6d36d62d64f) *(lore)* Route merged PRs to lore before CI fixes by @Christiantyemele in [#61](
https://github.com/NkwaTambe/openflows/pull/61)

  > - Changed vessel action priority: any_success (→lore) now routes before any_ci_fix (→forge)
  > - Added lore node to real_test.rs flow: ACTION_DEPLOYED routes to lore, not nexus
  > - Ensures docs/changelogs are written even when other PRs need CI fixes

- [b351ab2](
https://github.com/NkwaTambe/openflows/commit/b351ab229f504357f8d11922e7e5dbc9943e5699) *(nexus)* Rotate forge chats bound to stale/dead workspaces

  > create_chat_for_assignment previously treated a 'waiting' chat as active
  > even when it was bound to a deleted/re-provisioned workspace, so the forge
  > agent never actually connected to the live workspace and the ticket looped
  > 'in work' forever. Now the stored chat's workspace_id is compared against the
  > worker slot's current workspace_id; on mismatch the chat keys are cleared and
  > a fresh chat is created bound to the current workspace.
  >
  > Also includes:
  > - provision skills/standards/persona into worker workspaces via SSH after
  >   provisioning (agent-nexus + provisioner dep)
  > - openflows-forge template: launch the coding agent CLI (claude/codex) from a
  >   bind-mounted binary or PATH, with SessionStart hooks
  > - tenant clean/status/run commands: use tenant-scoped SharedStore and raw
  >   key scan/delete helpers to stop double-namespacing Redis keys
  > - agent-nexus: extra diagnostics around chat creation and workspace binding

- [2443ae5](
https://github.com/NkwaTambe/openflows/commit/2443ae572aa08043133ebecf1f2f398eee0182b0) *(orchestration)* Spawn SENTINEL for planning-gate review

  > When FORGE sets status to 'planning' and halts for SENTINEL gate approval,
  > the system was going stale because no mechanism spawned SENTINEL to review
  > the plan. The orchestration only spawned SENTINEL for 'review_ready' phase
  > (PR reviews), completely skipping the planning gate.
  >
  > Root cause: poll_harness_status_and_spawn_agents() only checked for
  > phase == 'review_ready', ignoring the 'planning' phase.
  >
  > Fixes:1. agent-nexus: Add planning-phase detection in poll_harness_status_and_spawn_agents()
  >    - When phase is 'planning', check for gate approval
  >    - If not approved, spawn SENTINEL chat with plan review instructions
  >    - SENTINEL reads PLAN.md and runs 'openflows-harness gate approve --phase planning'
  >
  > 2. agent-forge: Add ACTION_PLANNING_GATE signal
  >    - When harness status is 'planning', emit ACTION_PLANNING_GATE
  >    - Routes to NEXUS which triggers SENTINEL spawn
  >
  > 3. agent-sentinel: Handle planning_gate_pending verdicts
  >    - Check for approved gate approval in SharedStore
  >    - On gate approval, archive SENTINEL chat and release worker slot
  >
  > 4. Flow graph: Add ACTION_PLANNING_GATE route (forge_pair -> nexus)
  >
  > The planning gate workflow now works correctly:
  > - FORGE writes PLAN.md and sets status to 'planning'
  > - ForgePairNode emits ACTION_PLANNING_GATE
  > - Flow routes to NEXUS
  > - NEXUS detects planning phase and spawns SENTINEL
  > - SENTINEL reviews plan and runs 'openflows-harness gate approve --phase planning'
  > - FORGE can now transition to 'building' phase

- [025e3e4](
https://github.com/NkwaTambe/openflows/commit/025e3e471a6b051d0ca9ed9fb74171601d0a0042) *(uncategorized)* Improve quick-start UX, default env vars, and GitHub sign-ups

  > - Default REDIS_URL and CODER_URL in CLI instead of requiring them,
  >   so  and  work without extra .env
  >   configuration for the standard docker-compose setup.
  > - Enable Coder's built-in GitHub OAuth sign-ups via device flow
  >   (CODER_OAUTH2_GITHUB_ALLOW_SIGNUPS=true) — no OAuth app setup
  >   required. Coder ships with a default GitHub app for this.
  > - QUICK_START.md: clarify working directory, explain why each step
  >   matters, add direct Coder dashboard links (models, tokens,
  >   licenses), split env vars into required vs optional with
  >   defaults, remove long OAuth app setup instructions.
  > - .env.example: simplify CODER_SESSION_TOKEN instructions to a
  >   direct URL, add GitHub sign-ups documentation.

- [841b109](
https://github.com/NkwaTambe/openflows/commit/841b10985af7dc3ad115261b3847f8c64658f790) *(uncategorized)* Remove needless borrow flagged by clippy -D warnings

  > addr of a borrow in a store.del call

- [3b1322c](
https://github.com/NkwaTambe/openflows/commit/3b1322c87357fc70bbfd0d40bbec0918c1e212c3) *(uncategorized)* Agent orchestration pipeline - 6 critical bugs

  > Key fixes:
  > 1. SharedStore now tenant-aware (ns:{tenant}: prefix on all keys)
  >    - Nexus and Harness now use the same key namespace
  >
  > 2. Dispatch schema fixed - now includes ticket title/body
  >    - Agents receive actual task content instead of just metadata
  >
  > 3. Persona path corrected (missing /orchestration/ segment)
  >    - Personas from .agent.md files now load properly
  >
  > 4. Skills injected into initial prompt
  >    - load_skills_for_role() added, skills listed in prompt
  >
  > 5. Initial prompt enriched with full ticket context
  >    - Title and body embedded directly for immediate access
  >
  > 6. create_chat_for_assignment signature updated
  >    - Now accepts &Ticket to pass content through
  >    - Added create_chat_for_ticket_id() bridge method
  >
  > These fixes resolve the 'rogue agent' issue where agents had no
  > persona, broken dispatch/status commands, and no skills context.

- [cd300b5](
https://github.com/NkwaTambe/openflows/commit/cd300b5bf3527526927ebd4b8736cfc81c124df6) *(uncategorized)* Cycle nexus→forge_pair while workers are busy, not stop

  > nexus.exec returned no_work whenever no idle forge worker was available,
  > even when forge workers were actively building. This caused the controller
  > to stop after 3 consecutive no_work passes, abandoning in-progress work.
  >
  > Now nexus.exec distinguishes three states:
  > 1. No tickets, no idle workers, no busy workers → no_work (increment stop counter)
  > 2. Idle workers + assignable tickets → work_assigned (dispatch)
  > 3. No idle workers but busy forge workers → ACTION_EMPTY (cycle to forge_pair)
  >
  > The flow graph now routes nexus ACTION_EMPTY → forge_pair so the flow
  > cycles: nexus → forge_pair (monitors building chats) → empty → nexus →
  > (busy detected) → empty → forge_pair → ... until a PR opens or work fails.
  >
  > Ahened:_no_work_count only increments in case (1), so the 3-strike stop
  > fires only when there is truly nothing happening.
  >
  > Also raised workspace_ready timeout from 180s → 300s in bootstrap.rs and
  > lib.rs (workspaces take ~3m43s to provision on cold Docker starts).

- [75dbaa2](
https://github.com/NkwaTambe/openflows/commit/75dbaa2d9e9a2fcb27147d91545a027a2ff6e76e) *(uncategorized)* Break nexus<->forge_pair infinite loop and fix v2 registry slot derivation

  > The controller exhausted max_steps in a nexus <-> forge_pair ping-pong:
  > forge_pair returned ACTION_EMPTY (work in progress), the route map
  > bounced it back to nexus, and nexus re-dispatched the same ticket every
  > pass because nexus.exec hard-coded assign_to="forge-1" regardless of
  > worker-slot state, and recover_orphans reset Assigned tickets to Open
  > whenever the worker slot was merely Idle (always true for the phantom
  > "forge-1" that had no real WorkerSlot).
  >
  > Root cause was a v1/v2 registry field mismatch: the live registry.json
  > declares max_instances (v2) but forge_slots/all_worker_slots read the
  > v1 instances field (absent -> serde default 0), so zero forge slots
  > were ever provisioned. The old hard-coded "forge-1" operated on a
  > phantom worker with no workspace, no chat, and no GitHub token.
  >
  > Changes:- config/registry: add RegistryEntry::effective_instances() falling
  >   back to max_instances when instances is unset; use it in
  >   forge_slots(), all_worker_slots(), and total_instances().
  > - agent-nexus/exec: choose an actually-idle forge worker from
  >   worker_slots instead of hard-coding "forge-1"; return no_work
  >   (climbing the existing 3-strike stop) when no idle forge worker is
  >   available, so a single in-flight ticket no longer thrashes the flow.
  > - agent-nexus/recover_orphans: only reset an Assigned/InProgress
  >   ticket to Open when its worker slot is truly missing; idle-slot
  >   tickets are left to forge_pair chat monitoring.
  > - agent-nexus/post: log consecutive_no_work/threshold each pass.
  > - agent-forge/post_batch: emit a summary log (monitored, has_pr_opened,
  >   has_failed, has_in_progress) before deciding the action.
  > - agentflow: raise max_steps(20) to max_steps(1000) so a real backlog
  >   no longer trips the safety cap as a false infinite loop.
  > - gitignore: exclude .dev-binaries/ build artifacts.
  >
  > Also includes the pre-existing Coder-only redesign refactor (removal
  > of agent-client, agentflow-tui, pair-harness, e2e tests, stale docs)
  > that was already present in the working tree.
  >
  > Verified:cargo check -p openflows -p config -p agent-nexus -p
  > agent-forge -p pocketflow-core clean; cargo test -p config -p
  > agent-nexus -p agent-forge -p pocketflow-core all passing.

- [92a4430](
https://github.com/NkwaTambe/openflows/commit/92a4430d6d492e04a56c2e9b7f36aa1bee9a9ffa) *(uncategorized)* Use --env-file /dev/null for docker-compose to avoid parsing project .env

  > Docker-compose reads .env from the project root by default, which contains
  > API keys with characters that its parser can't handle (parentheses, commas).
  > Pass --env-file /dev/null to skip that, and provide only the Coder-specific
  > vars (CODER_URL, CODER_ADMIN_PASSWORD, CODER_PG_PASSWORD) via -e flags.

- [dac3c02](
https://github.com/NkwaTambe/openflows/commit/dac3c025f1d97512eabb0786a00e962e513a5edc) *(uncategorized)* Add Coder bootstrap and docker-compose auto-start to running binary

  > The Coder integration code was in binary/src/main.rs but the actual
  > running binary is binary/src/bin/agentflow.rs. This meant choosing Coder
  > mode in setup had no effect at runtime.
  >
  > Changes:- Move Coder bootstrap logic into agentflow.rs (the actual binary)
  > - Auto-start Coder docker-compose services when CODER_URL is set
  >   but Coder server is unreachable
  > - Search for docker-compose.yml in CWD, ~/.openflows/, and project root
  > - Better user feedback: clear messages when Coder bootstrap fails,
  >   with actionable steps
  > - Handle WORKSPACE_PROVIDER=coder without CODER_URL by setting default
  > - Store CODER_API_TOKEN and CODER_URL in SharedStore for downstream
  >   nodes (nexus, vessel) to reconstruct CoderClient
  > - Also check WORKSPACE_PROVIDER env var as Coder mode indicator

- [2e342e0](
https://github.com/NkwaTambe/openflows/commit/2e342e07c696e804fbda6818835716a3a86e95fe) *(uncategorized)* Vessel node now uses resolver registry and searches OPENFLOWS_HOME first

  > - agentflow.rs: Use VesselConfig::from_registry() with the resolver path
  >   instead of VesselNode::from_env() which searched only CWD
  > - VesselNode::from_env(): Search OPENFLOWS_HOME first for registry,
  >   then workspace root, then CWD — same priority as OrchestrationResolver
  > - VesselConfig::from_env(): Improved error message to mention per-agent
  >   tokens as well as global PAT

- [3b56058](
https://github.com/NkwaTambe/openflows/commit/3b56058496baaf32a27fca3aec244abeec44e374) *(uncategorized)* Comprehensive defense against registry/token/path bugs

  > 1. OrchestrationResolver: reject candidates ending in 'orchestration'
  >    (not just 'orchestration/agent') to prevent doubled paths like
  >    /foo/orchestration/orchestration/agent/registry.json
  >
  > 2. resolve_github_token: bail for inactive agents instead of silently
  >    falling back to GITHUB_PERSONAL_ACCESS_TOKEN. The original code
  >    would succeed for inactive agents if the global PAT existed,
  >    masking misconfiguration (the exact bug the user hit).
  >
  > 3. Setup TUI: read registry from OPENFLOWS_HOME first in all three
  >    setup steps (step_agents, step_github, step_existing), not just
  >    from CWD. Previously if you ran setup from a directory other than
  >    ~/.openflows, it would find stale bundled defaults instead of
  >    your customizations.

- [d1cedb9](
https://github.com/NkwaTambe/openflows/commit/d1cedb94b642026db320b27a98f20821267d61e1) *(uncategorized)* Reject candidates inside orchestration/agent to prevent doubled paths

  > When the CWD or a candidate path was inside an orchestration subdirectory
  > (e.g. ~/.openflows/orchestration/agent/), the resolver would match it,
  > causing orchestrator_dir to become that subdirectory. Then
  > ensure_orchestration_dir() would write to
  > orchestrator_dir/orchestration/agent/registry.json, producing the
  > doubled path ~/.openflows/orchestration/agent/orchestration/agent/registry.json

- [1de1ca7](
https://github.com/NkwaTambe/openflows/commit/1de1ca7d295da754ccc5d1533725fe30382a26e6) *(uncategorized)* Address PR reviews — backup from OPENFLOWS_HOME, extract helper, add reset recovery hint

  > - install.sh: extract install_orchestration() helper, backup/restore
  >   registry.json from $OPENFLOWS_HOME (~/.openflows) not $INSTALL_DIR
  > - install.sh: deduplicate backup logic from both download_binary()
  >   and build_from_source()
  > - orchestration.rs: add recovery hint to --reset-orchestration log
  >   telling users to delete registry.json and re-run if reset needed

- [3a088cf](
https://github.com/NkwaTambe/openflows/commit/3a088cfd1c0aff9db5847b629e4058d2cf791c6a) *(uncategorized)* Preserve user registry.json during install and preserve on reset

  > - Install script now backs up and restores registry.json so user
  >   customizations (like lore.active: false) are preserved across
  >   upgrades
  > - --reset-orchestration now skips registry.json if it exists,
  >   preventing it from discarding user agent configuration
  > - Simplified current_exe() to single call in OrchestrationResolver

- [08122fd](
https://github.com/NkwaTambe/openflows/commit/08122fd8b787be66f7e12d874ea85d1b9f99ac64) *(uncategorized)* Prioritize OPENFLOWS_HOME registry and gracefully skip lore on token error

  > Two bugs fixed:
  >
  > 1. OrchestrationResolver registry resolution now prioritizes
  >    OPENFLOWS_HOME (~/.openflows) as the first search candidate.
  >    Previously, stale registries at binary/CWD paths could override
  >    user setup customizations (e.g. lore.active: false), causing the
  >    runtime to find lore.active:true from an older bundled registry.
  >
  > 2. LoreNode initialization now gracefully degrades instead of crashing.
  >    When lore is active but its AGENT_LORE_GITHUB_TOKEN is missing,
  >    the binary logs a warning and skips lore rather than propagating
  >    the error with ? and terminating the entire process.
  >
  > Also adds a diagnostic log when an existing registry is found, and
  > filters out empty PathBuf entries from failed current_exe() calls.

- [522cd6a](
https://github.com/NkwaTambe/openflows/commit/522cd6a7b91f8ccc560de5456dfd4cc409057193) *(uncategorized)* Set executable permissions on .sh files and restore trap in install.sh

  > - After writing bundled .sh files, set 0o755 permissions (Unix only)
  >   so hooks are executable by the orchestration system
  > - Same in reset_orchestration_dir()
  > - Restore 'trap ... RETURN' in build_from_source() so temp dir
  >   is cleaned up on error paths too

- [0ce12b4](
https://github.com/NkwaTambe/openflows/commit/0ce12b4f6a38357aa0874dafb8b856a4242b4c1c) *(uncategorized)* Address gitar review feedback

  > - Fix stale-version detection: read disk version BEFORE writing .version
  >   file, so the warning actually fires when on-disk version differs
  > - Remove unused skipped_custom variable from reset_orchestration_dir()
  > - Align unknown arg handling: both binaries now print usage and exit(0)

- [505d114](
https://github.com/NkwaTambe/openflows/commit/505d1148f6befe1305ac096c634a4601b01dfd75) *(uncategorized)* Track all orchestration files in git and remove duplicate profile

  > - Remove blanket 'orchestration/' from .gitignore (was overriding
  >   the !orchestration/plugin/ and !orchestration/agent/ negations,
  >   causing registry.json and other files to be excluded from CI)
  > - Force-add orchestration/agent/registry.json to git tracking
  > - Remove [profile.release] from binary/Cargo.toml (was causing
  >   'profiles for non root package will be ignored' CI warning)

- [e3783f0](
https://github.com/NkwaTambe/openflows/commit/e3783f0e3d8f908f1a1ba29595913cc8adb970d9) *(uncategorized)* OPENFLOWS_HOME used directly without double-appending /.openflows in load_env()

  > When OPENFLOWS_HOME is set (e.g. /home/user/.openflows), load_env() was
  > producing paths like /home/user/.openflows/.openflows/.env because the
  > .map() appended /.openflows to all three sources including OPENFLOWS_HOME
  > itself. Now OPENFLOWS_HOME is used as-is, and /.openflows is only
  > appended to HOME/USERPROFILE fallbacks — matching main.rs, step_done.rs,
  > step_existing.rs, and the orchestrator_dir resolution.

- [5ae940b](
https://github.com/NkwaTambe/openflows/commit/5ae940bca492ed6a9baf8f64891e68230a1050d7) *(uncategorized)* Standardize on OPENFLOWS_HOME, propagate .env parse errors, remove /tmp fallback

  > - Replace AGENTFLOW_HOME with OPENFLOWS_HOME consistently across all
  >   binaries for both .env loading and orchestrator_dir resolution
  > - load_env() in agentflow.rs and demo.rs now properly propagates
  >   .env parse errors instead of silently swallowing them
  > - orchestrator_dir candidate search only adds a home-based path when
  >   OPENFLOWS_HOME or HOME is resolvable — no /tmp fallback
  > - Setup wizard now writes registry.json to both current dir and
  >   ~/.openflows/ so the binary can find it regardless of cwd
  > - Removed unused std::path::Path import in step_done.rs
  > - Ran cargo fmt --all

- [5b120b0](
https://github.com/NkwaTambe/openflows/commit/5b120b075a9400a909e3a66ff75f7ae8e4060c7f) *(uncategorized)* Load .env from ~/.openflows, install orchestration config, and improve Quick Start

  > - All binaries now check ~/.openflows/.env (or $OPENFLOWS_HOME/.env)
  >   before falling back to current directory .env
  > - Setup wizard writes .env to ~/.openflows/ instead of cwd
  > - Doctor and existing-config detection check ~/.openflows/.env
  > - Install scripts copy orchestration/ directory alongside binaries
  > - npm installer moves orchestration/ to package root
  > - orchestrator_dir resolution searches: binary dir, binary parent
  >   (npm layout), ~/.agentflow, cwd — in order
  > - README Quick Start expanded with binary, npm, and source install
  >   sections

- [0a8af31](
https://github.com/NkwaTambe/openflows/commit/0a8af31bf32be131a7497804d52ab166e02d3e5f) *(uncategorized)* Create temp registry.json in nexus e2e test instead of reading from workspace

  > The test depended on ../orchestration/agent/registry.json which is not
  > tracked by git and doesn't exist in CI. Now the test creates a temporary
  > registry file with the minimum required configuration, making it self-contained.
  >
  > Also fix nexus_real_e2e.rs to use the same approach.

- [a1acbaf](
https://github.com/NkwaTambe/openflows/commit/a1acbafa68521ec70d173237d6c2d57ba00e4f6f) *(uncategorized)* Resolve nexus e2e test by using CARGO_MANIFEST_DIR for workspace-relative paths

  > The test used relative paths like '../orchestration/agent/registry.json' which
  > only work when run from the binary/ directory. In CI, tests run from
  > target/debug/deps/ so relative paths fail. Use CARGO_MANIFEST_DIR to resolve
  > paths relative to the workspace root instead.

- [da26aaa](
https://github.com/NkwaTambe/openflows/commit/da26aaa97672cafa8684de87b20bacb44e7dbdc4) *(uncategorized)* Clippy warnings, format, and test fixes for CI

  > - Fix doc comment indentation in strip_provider_prefix
  > - Fix manual Range::contains clippy lint
  > - Remove unnecessary format! and add #[allow] for pre-existing warnings
  > - Restore to_ascii_lowercase() in Gemini normalize_model_name
  > - Run cargo fmt across workspace
  > - Add #[allow(dead_code)] for unused discovery functions

- [04b5b67](
https://github.com/NkwaTambe/openflows/commit/04b5b671f2a615f76435d6050e0963464e95d38b) *(uncategorized)* Implementing plugin module for codex by @ndefokou in [#64](
https://github.com/NkwaTambe/openflows/pull/64)

- [5398d0b](
https://github.com/NkwaTambe/openflows/commit/5398d0be126b9a4c8fda6d61d3b6846a65789e55) *(uncategorized)* Resolve clippy warnings - implement FromStr trait and remove dead code

- [26eb85c](
https://github.com/NkwaTambe/openflows/commit/26eb85c5ff7f749bb118600e29691380127edd86) *(uncategorized)* Resolve CI failures (format, clippy, license) by @Christiantyemele in [#61](
https://github.com/NkwaTambe/openflows/pull/61)

- [7b73c94](
https://github.com/NkwaTambe/openflows/commit/7b73c944b73070c56dd845af6d6bd5cf281cdc05) *(uncategorized)* Resolve clippy, fmt, cargo-deny, and semver-checks CI failures by @Christiantyemele in [#31](
https://github.com/NkwaTambe/openflows/pull/31)

  > - Clippy: replace redundant closures, use is_multiple_of, derive Default,
  >   collapsible match guard, is_none_or/is_some_and, &mut [_] slice
  > - Format: cargo fmt applied
  > - Cargo Deny: add MIT license to agent-vessel Cargo.toml
  > - Semver: add #[non_exhaustive] to TicketStatus, TestResults, Contract;
  >   restore backward-compat remove_worktree(pair_id) and spawn_sentinel
  >   (5-param) overloads

- [657fd83](
https://github.com/NkwaTambe/openflows/commit/657fd83772c5b496fe7c70ab167bf8a6c846ad7a) *(uncategorized)* Resolve merge conflict infinite loop — unrelated histories, force-push, duplicate PR prevention by @Christiantyemele in [#31](
https://github.com/NkwaTambe/openflows/pull/31)

  > The VESSEL→FORGE conflict resolution loop was stuck because:
  >
  > 1. git merge origin/main refused to merge unrelated histories (worktree
  >    branch and main don't share a common ancestor), preventing FORGE from
  >    seeing conflict markers locally. Fix: retry with --allow-unrelated-histories
  >    in both pair-harness/worktree.rs and agent-vessel/node.rs.
  >
  > 2. After a clean merge during conflict rework, the merge commit was only
  >    local — never pushed to remote, so GitHub still saw the PR as conflicting.
  >    Fix: force-push branch with --force-with-lease after clean merge.
  >
  > 3. push_and_create_pr always created a NEW PR even when one already existed
  >    for the branch, producing duplicates (PR #40 still open, #41 created).
  >    Fix: check for existing open PRs before creating; use --force-with-lease
  >    when normal push is rejected (non-fast-forward).
  >
  > 4. FORGE agent prompts didn't mention force-with-lease or duplicate PR
  >    avoidance. Fix: updated conflict rework TASK.md and PR creation prompts.
  >
  > 5. vessel node.rs had a latent panic: fetch.is_err() || fetch.unwrap()
  >    would panic on Err. Fix: proper match-based error handling.

- [61f97f1](
https://github.com/NkwaTambe/openflows/commit/61f97f1ac3b2b068ec215e2c784c79387351debf) *(uncategorized)* Add ticket state machine, remove nested tokio runtime, and fix worktree cleanup by @Christiantyemele in [#12](
https://github.com/NkwaTambe/openflows/pull/12)

  > - Add TicketStatus enum (Open/Assigned/InProgress/Failed/Completed/Exhausted)
  >   with retry budget (MAX_ATTEMPTS=3) to prevent infinite retry loops
  > - Nexus now tracks and provides assignable_tickets to LLM context,
  >   fixing the 'no open issues' bug after forge failure
  > - Remove nested tokio::runtime inside spawn_blocking in ForgePairNode,
  >   which caused FORGE process spawn failures
  > - WorktreeManager: make git fetch/merge non-fatal, prune stale worktrees,
  >   delete associated branches on removal, and clean conflicting branches
  >   before creation

### Refactor

- [44c2184](
https://github.com/NkwaTambe/openflows/commit/44c21842fb8cca5e65044739175914b2469d9e03) *(uncategorized)* Rename orchestration volume to artifacts

  > The shared Docker volume openflows-orchestation-<tenant> and its mount
  > path /home/coder/.openflows/orchestration/ carried a misleading name:
  > 'orchestration' suggests it belongs to the orchestrator/nexus only, when
  > in fact it is the shared directory where forge writes PLAN.md and
  > sentinel reads it — the primary forge↔sentinel coordination channel.
  >
  > Rename to openflows-artifacts-<tenant> and mount path
  > /home/coder/.openflows/artifacts/ — 'artifacts' accurately
  > communicates that this is the shared artifact exchange directory
  > for agent coordination (plans, skills, standards, personas).
  >
  > Changes:- Templates (nexus, forge, sentinel): volume name, mount path, comments
  > - Nexus template: ORCHESTRATOR_DIR env var → ARTIFACTS_DIR
  > - agent-nexus lib.rs: env var reads from ORCHESTRATOR_DIR → ARTIFACTS_DIR
  > - agentflow.rs: env var set from ORCHESTRATOR_DIR → ARTIFACTS_DIR

- [e8b1faa](
https://github.com/NkwaTambe/openflows/commit/e8b1faa17b57faa6be215c05bc548fcc4908365f) *(uncategorized)* Call current_exe() once and derive parent dirs from single result

  > Address PR review feedback — avoids two separate OS syscalls and
  > potential inconsistency between invocations.

- [966d939](
https://github.com/NkwaTambe/openflows/commit/966d9390d22dff7a29d921388ae6a68c1904d9d7) *(uncategorized)* Rename all agentflow binaries to openflows

  > Renamed binaries:
  > - agentflow → openflows
  > - agentflow-setup → openflows-setup
  > - agentflow-dashboard → openflows-dashboard
  > - agentflow-doctor → openflows-doctor
  > - demo → openflows-demo
  >
  > Updated files:
  > - binary/Cargo.toml - [[bin]] target names
  > - Makefile - BINARIES variable and all references
  > - scripts/install.sh - binary list and commands
  > - scripts/check_setup.sh - cargo check command
  > - scripts/start_proxy.sh - comment reference
  > - scripts/demo_recording_guide.md - cargo run commands
  > - Dockerfile - COPY commands, ENTRYPOINT, HEALTHCHECK
  > - .github/workflows/ci.yml - build and upload commands
  > - .github/workflows/release.yml - build and tarball commands
  > - tests/e2e/smoketest.sh - cargo run commands
  > - All documentation (INSTALL.md, RUN.md, TUTORIAL.md, BUILD.md, DEMO.md,
  >   CONTRIBUTING.md, PACKAGING.md, docs/*.md)
  > - packaging/npm/ - install.js and bin wrapper scripts
  > - packaging/homebrew/ - test assertion
  >
  > Binary names now consistently match the project name 'openflows'.

### Styling

- [baf7855](
https://github.com/NkwaTambe/openflows/commit/baf78553dbb33f5e9588670787b0aefbc7554d32) *(doctor)* Wrap long println to satisfy rustfmt by @NkwaTambe in [#220](
https://github.com/NkwaTambe/openflows/pull/220)

- [85d3ce2](
https://github.com/NkwaTambe/openflows/commit/85d3ce24cf94a83b0ad0fe9c0f8d221d6c7f4af8) *(uncategorized)* Fix formatting and trailing whitespace

- [4dbad79](
https://github.com/NkwaTambe/openflows/commit/4dbad79586bcd471d732ec5afa879ef508ab88c4) *(uncategorized)* Fix formatting for CI

- [301c590](
https://github.com/NkwaTambe/openflows/commit/301c590341d23fe6fb0382094ebd45072e3b8735) *(uncategorized)* Apply cargo fmt formatting

### Testing

- [d2212da](
https://github.com/NkwaTambe/openflows/commit/d2212da4b486e1a2c70b52ec96a8a944c8575aca) *(uncategorized)* Add codex E2E test variants for nexus

  > - nexus_e2e_codex.rs: mocked test with OpenAI /chat/completions endpoint
  > - nexus_real_e2e_codex.rs: real E2E test (ignored) for codex backend
  > - Both use registry.json with default_cli=codex and openai/gpt-4o-mini
  > - Mock mocks /chat/completions (OpenAI format) instead of /v1/messages
  > - Sets LLM_PROVIDER=openai, OPENAI_BASE_URL, removes ANTHROPIC vars

### Miscellaneous Tasks

- [00ae71d](
https://github.com/NkwaTambe/openflows/commit/00ae71d73acee3620bd78c4e39705273d20df99a) *(coder)* Update non-code refs to org-scoped v2 models ([#186](https://github.com/The-AgenticFlow/openflows/pull/186))

- [1be6c5a](
https://github.com/NkwaTambe/openflows/commit/1be6c5a9d5ad7125f3f98b7b9da0487147a13f4b) *(openflows)* Release v1.3.0 by @github-actions[bot] in [#236](
https://github.com/NkwaTambe/openflows/pull/236)

- [bd1a946](
https://github.com/NkwaTambe/openflows/commit/bd1a946233a4881c6a7aedaefccbdcd629d7950a) *(uncategorized)* Bump openflows and openflows-harness to 1.2.1

- [c3c8df3](
https://github.com/NkwaTambe/openflows/commit/c3c8df3fb0fddeebe40b60e0c84be9a867d842bd) *(uncategorized)* Bump version to 1.0.16

- [453c69b](
https://github.com/NkwaTambe/openflows/commit/453c69bcd9e685f68e22ca85ef7bfa55e0be542c) *(uncategorized)* Project cleanup, rename binary to agentflow, and update docs

  > - Rename binary from real_test to agentflow with additional demo targets
  > - Remove legacy src/ files (main.rs, nodes.rs, state.rs, utils.rs)
  > - Update README with registry system documentation and architecture diagram
  > - Add BUILD.md and RUN.md guides
  > - Add example issues documentation and init-target-project script
  > - Update CONTRIBUTING, DEMO, and TUTORIAL docs
  > - Update Cargo.toml with new binary targets

### Continuous Integration

- [332a56d](
https://github.com/NkwaTambe/openflows/commit/332a56df7d13c0c48a9eb1f707b4d14adde324a9) *(uncategorized)* Retry CI checks

### Hardening

- [5051a7a](
https://github.com/NkwaTambe/openflows/commit/5051a7ae0fa97f37a6c9d7fbc9b15528b3c3aa8c) *(uncategorized)* Shared dir in worktree, MCP timeouts, Claude provisioning, RUST_LOG support

  > - Move shared directory from orchestration/pairs/{pair}/{ticket}/shared
  >   to worktrees/{pair}/.pair-shared — required for Codex workspace-write
  >   sandbox (--add-dir bug in v0.130.0 bypassed by in-worktree placement)
  > - Add MCP request timeout (30s default, MCP_REQUEST_TIMEOUT_SECS env)
  >   with session disconnect detection and subprocess kill on hang
  > - Enable Claude backend provisioning (needs_extras_provisioning: true)
  >   and remove --add-dir flags for both Claude and Codex backends
  > - Add --skip-git-repo-check for Codex sentinel (.pair-shared is not
  >   a git repo root)
  > - Support RUST_LOG env var for tracing level override in both binaries
  > - Update registry model backend to glm-5p1
  > - Update shared dir path in vessel node and pair harness test
  > - Add pair harness documentation

### Merge

- [035c6b6](
https://github.com/NkwaTambe/openflows/commit/035c6b6446fd15e94c5948925a9bf8e85b1f246e) *(uncategorized)* Resolve conflicts with main, preserving all VESSEL agent and main features by @Christiantyemele in [#31](
https://github.com/NkwaTambe/openflows/pull/31)

### Release

- [cc673c6](
https://github.com/NkwaTambe/openflows/commit/cc673c68a2afd575de46fef78395b9d55c05d888) *(uncategorized)* V1.1.8 — fix worker workspaces booting without openflows-harness

  > The v1.1.6 and v1.1.7 GitHub releases that worker templates (forge,
  > sentinel, vessel, lore) download openflows-harness from no longer exist
  > (likely removed after creation), so the pinned-version curl download in
  > every worker's startup_script has been silently 404ing. The script logs
  > a WARNING and continues, so workspaces boot with no harness binary and
  > get handed ticket dispatch anyway (observed on T-047, T-048 sentinel
  > sessions).
  >
  > - Bump binary/Cargo.toml to 1.1.8 so the release workflow builds and
  >   publishes a fresh openflows-harness binary under this tag.
  > - Bump harness_version default in all four worker templates
  >   (openflows-forge, openflows-sentinel, openflows-vessel, openflows-lore)
  >   from 1.1.6 to 1.1.8 so newly provisioned workspaces download the binary
  >   that will actually exist after this release ships.
  >
  > Follow-up (not in this commit): make the startup_script fail hard (or
  > block ticket dispatch) instead of warning-and-continuing when the harness
  > download fails, so this class of silent-degradation can't recur.

- [ba1980c](
https://github.com/NkwaTambe/openflows/commit/ba1980cb05e89ff08444ddfd15dbcd4dfe908b54) *(uncategorized)* Bump version to 1.1.6





