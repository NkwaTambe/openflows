# Coder v2.37.0 GA Migration — Implementation Plan

Epic: [Migrate OpenFlows to Coder v2.37.0 GA](https://github.com/The-AgenticFlow/openflows/issues/186)

## Why now

Coder promoted **Coder Agents (incl. the Chats API) to GA in v2.37.0** (released 01 Sep 2026).

OpenFlows' `coder-client` still targets the **experimental** Chats API, which is being
removed:

| Legacy (experimental) | GA (v2.37.0) |
| --- | --- |
| `/api/experimental/chats/models` | `/api/v2/organizations/{organization}/chats/models` |
| `/api/experimental/chats/model-configs` | removed — folded into the org models response |
| `/api/experimental/chats` (lifecycle) | `/api/v2/chats` |
| `organization_id` optional in body | `organization_id` **required** in body |

OpenFlows runs `ghcr.io/coder/coder:${CODER_IMAGE_TAG:-latest}` (`docker-compose.yml:29`),
and `:latest` already resolves to v2.37.0 — so the **models endpoint is failing today**,
and all chat endpoints fail when v2.38 ships.

- The default-organization model routes (`/api/experimental/chats/models`,
  `/api/experimental/chats/model-configs`) are removed **immediately** in v2.37.
- The `/api/experimental/chats` lifecycle routes remain only for a one-month migration
  window, removed in v2.38.
- Model configurations and overrides are now **organization-scoped**; the separate
  model-availability endpoint was removed and folded into the models response.

## Ticket breakdown (from the epic)

| # | Ticket | Blocked by |
| --- | --- | --- |
| T1 | Migrate `coder-client` Chats API to `/api/v2` (lifecycle + events + org-scoped models) | None (URGENT — models endpoint already broken on `:latest`) |
| T2 | Update mock chat server + `coder-client` tests to v2 paths | T1 |
| T3 | Update non-code references (registry.rs, types.rs, doctor.rs, docs) | T1 |
| T4 | Pin & validate Coder version (replace `:latest`) | T1 |
| T5 | Assess remaining v2.37.0 breaking changes (MCP / model-config / external-auth / OAuth) | None |
| T6 | Full integration verification on pinned Coder v2.37.0 | T1–T5 |

## Implementation sequence (issue by issue)

### T1 — Migrate `coder-client` Chats API to `/api/v2`

All work in `crates/coder-client/src/lib.rs`, plus `chat_stream.rs` and `types.rs`.

1. **Org-scoped models endpoint** (`lib.rs`):
   - Change `list_chat_models()` (both `#[cfg(feature="chats-api")]` and the no-cache
     variant) to hit `GET /api/v2/organizations/{org}/chats/models`.
   - Resolve `{org}` via the existing `get_default_organization_id()` (already present
     in this file and cached). Handle cache invalidation on org change.
   - Update `parse_chat_models_body(&body)` (lib.rs:66, around line 66-…) — the v2
     models response is org-scoped and no longer has a separate availability endpoint;
     the shape should already be parsable but must be re-verified against the v2 body
     (providers[] + models[]). Keep the flattening tests passing; add v2 fields if the
     response shape changed (e.g. `tags`, `capabilities`).

2. **Chat lifecycle endpoints** (`lib.rs`, Chats API impl block):
   - `POST /api/experimental/chats` → `POST /api/v2/chats` (create_chat)
   - `GET /api/experimental/chats/{chat}` → `GET /api/v2/chats/{chat}` (get_chat, get_chat_opt)
   - `GET /api/experimental/chats` → `GET /api/v2/chats` (list_chats)
   - `POST /api/experimental/chats/{chat}/messages` → `POST /api/v2/chats/{chat}/messages`
   - `GET /api/experimental/chats/{chat}/messages` → `GET /api/v2/chats/{chat}/messages`
   - `PATCH /api/experimental/chats/{chat}` → `PATCH /api/v2/chats/{chat}` (archive)
   - `POST /api/experimental/chats/{chat}/interrupt` → `POST /api/v2/chats/{chat}/interrupt`

3. **Organization ID required**: In v2, `organization_id` is **required** in the
   create-chat body and the caller must be a member. In `create_ticket_chat()`
   (lib.rs convenience methods), the org ID is already resolved via
   `get_default_organization_id()`. Make `CreateChatRequest.organization_id` a
   guaranteed value before `create_chat` — escalate `None` to a hard error rather
   than `warn` + `None` for the v2 path (or keep the fallback but ensure the client
   always supplies an org id).

4. **Events WebSocket** (`chat_stream.rs:107-112`):
   - `/api/experimental/chats/{chat}/events` → `/api/v2/chats/{chat}/events` (two
     places: ws-prefixed and http→ws rewrite branches).

5. **ModelInfo / types** (`types.rs`):
   - `CreateChatRequest.organization_id` doc comment (types.rs:365): "Required by the
     Coder experimental chats API" → update to v2 GA wording and mark required.
   - `ModelInfo` doc (types.rs:427) references `/api/experimental/chats/models` →
     update to the org-scoped v2 path.

Verification: `cargo build -p coder-client`, `cargo test -p coder-client`.

### T2 — Update mock chat server + `coder-client` tests to v2 paths

All work in `crates/coder-client/src/mock_chat_server.rs` and test modules.

1. `mock_chat_server.rs:4` module doc still says `/api/experimental/chats/{id}/events`
   → update to `/api/v2/chats/{id}/events`. The mock binds a bare `ws://127.0.0.1:{port}`
   so no path routing is required, but the doc/assertions referencing experimental
   paths must be updated.
2. Re-verify `ChatStream::connect` tests that build the v2 URL (in `chat_stream.rs`
   tests or `lib.rs` tests) so they assert the `/api/v2/chats/{id}/events` URL.
3. Add a unit test for `list_chat_models()` hitting the org-scoped path (mock the
   `/api/v2/organizations/{org}/chats/models` response and assert `get_default_organization_id`
   is called + models parsed).
4. Add coverage asserting `create_chat` sends a required non-empty `organization_id`.

### T3 — Update non-code references

1. `crates/config/src/registry.rs`:
   - registry.rs:177 comment: "v2: Coder model hint (matched against
     GET /api/experimental/chats/models)" → update to org-scoped v2 path.
   - Check `.model` hint semantics still valid (model hint is only a hint; the server
     resolves it) — confirm no code change, just comment.
2. `binary/src/doctor.rs` (and `binary/src/bin/doctor.rs`):
   - doctor.rs:61 uses `{}/api/experimental/chats/models` for the LLM-models check →
     update to `{}/api/v2/organizations/{org}/chats/models`. Requires resolving the org
     id (reuse `get_default_organization_id`, or add a lightweight helper). Update the
     async function signature if it needs an org id / client.
   - Update message text "check Coder dashboard → AI Settings" if applicable.
3. **Docs**: create/refresh this `docs/architecture/coder-v2.37-migration.md` (this
   file) as the source of truth; add cross-references and the pin note.
4. Grep for any remaining `experimental` references in docs/ and non-coder-client code
   and update or annotate.

### T4 — Pin & validate Coder version (replace `:latest`)

1. `docker-compose.yml:29`:
   `image: ghcr.io/coder/coder:${CODER_IMAGE_TAG:-latest}` →
   `image: ghcr.io/coder/coder:${CODER_IMAGE_TAG:-v2.37.0}`.
2. Update the `.env.example` / README / QUICK_START that documents
   `CODER_IMAGE_TAG` (default-latest references) to reflect `v2.37.0`.
3. Add a health/validation step (doctor or CI) confirming the running Coder reports
   `v2.37.0` via `/api/v2/buildinfo` (doctor.rs already checks buildinfo) and that the
   org-scoped models endpoint responds.

### T5 — Assess remaining v2.37.0 breaking changes (MCP / model-config / external-auth / OAuth)

Research + targeted fixes as discovered (primarily docs + config wiring review):

- **model-config**: the separate experimental model-config routes are removed; verify no
  OpenFlows code calls `/api/experimental/chats/model-configs` (none found — confirmed by
  repo grep). Record in this doc.
- **MCP**: the epic lists MCP as an area to assess. Confirm whether OpenFlows provisions
  agent MCP servers (templates / coder modules). If so, validate against v2.37 behavior.
- **external-auth / OAuth**: `docker-compose.yml` has commented `CODER_EXTERNAL_AUTH_*`
  stanzas (lines 46-53). Verify no openflows code depends on external-auth or OAuth
  endpoints that changed in v2.37. The epic notes OAuth now rejects unsupported /
  over-broad scopes — flag any GitHub OAuth scope usage for review.
- Produce a short "no action / action" table appended to this doc.

### T6 — Full integration verification on pinned Coder v2.37.0

- Spin up `docker compose up` with `CODER_IMAGE_TAG=v2.37.0`.
- Run `binary/doctor` (updated in T3) end-to-end: buildinfo + org-scoped models check.
- Exercise the full chat flow against the pinned server:
  1. `list_chat_models()` returns org-scoped models.
  2. `create_ticket_chat()` / `create_chat()` succeeds with required org id.
  3. `send_chat_message()` + `get_chat_messages()` work.
  4. `ChatStream` events flow over `/api/v2/chats/{id}/events`.
  5. archive + interrupt.
- Run the full Rust test suite (`cargo test --workspace`) and address any drift.

## AI usage declaration

This epic and its tickets were drafted with AI assistance. Facts (release version,
endpoint paths, removal timeline) were verified against primary `coder/coder` sources
and are cited inline. No claim is asserted on assumption; anything unverifiable is
excluded. Human review required before any code changes land.
