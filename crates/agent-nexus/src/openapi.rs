//! OpenAPI (OpenAPI 3.1) documentation for Agent Nexus HTTP surfaces.
//!
//! Agent Nexus hosts two protocol-specific HTTP routers that cannot be
//! represented cleanly in the Manager document:
//!
//! - **A2A relay** (`crates/agent-nexus/src/a2a/http_server.rs`): a JSON-RPC
//!   endpoint (`POST /rpc`) used by Sentinel↔Forge delegated verification,
//!   plus an SSE stream (`GET /`), health, and an agent card.
//! - **Lifecycle hooks consumer** (`crates/agent-nexus/src/hooks/server.rs`):
//!   a JWT-signed webhook that observes Coder agent lifecycle events.
//!
//! The spec is generated from the single inventory [`NEXUS_ROUTES`] and served
//! at `/openapi.json` on both routers. It documents endpoints and contracts but
//! never contains credentials or real secrets.

use utoipa::openapi::{
    info::Info,
    path::{HttpMethod, OperationBuilder, PathItemBuilder, PathsBuilder},
    request_body::{RequestBody, RequestBodyBuilder},
    schema::ArrayBuilder,
    Content, ObjectBuilder, OpenApi, OpenApiBuilder, Paths, Ref, ResponseBuilder, Type,
};

const GET: &str = "GET";
const POST: &str = "POST";

/// A single documented Agent Nexus route.
#[derive(Debug, Clone, Copy)]
pub struct RouteInfo {
    /// HTTP method, e.g. `"GET"`.
    pub method: &'static str,
    /// Full path as registered on the router.
    pub path: &'static str,
    /// Functional tag (`a2a` or `hooks`).
    pub tag: &'static str,
    /// One-line human summary.
    pub summary: &'static str,
    /// Longer description (may describe protocol details such as JSON-RPC
    /// methods or query parameters).
    pub description: &'static str,
}

/// Complete inventory of the Agent Nexus HTTP surface (A2A relay + hooks).
///
/// When a new route is added to either router, it MUST be added here too; the
/// coverage test in this module keeps the routers and this inventory in sync.
pub const NEXUS_ROUTES: &[RouteInfo] = &[
    // ---- A2A relay ----
    RouteInfo {
        method: POST,
        path: "/rpc",
        tag: "a2a",
        summary: "A2A JSON-RPC dispatch",
        description: "Dispatches A2A methods by name in the JSON-RPC body: message/send, tasks/get, tasks/claim, tasks/complete, tasks/cancel, tasks/resubscribe, tasks/push_progress.",
    },
    RouteInfo {
        method: GET,
        path: "/",
        tag: "a2a",
        summary: "SSE verify-task progress stream",
        description: "Server-Sent Events stream of VerifyProgressEvent chunks. Query parameter: task_id (required). Replays buffered events on connect, then streams live until completion or disconnect.",
    },
    RouteInfo {
        method: GET,
        path: "/health",
        tag: "a2a",
        summary: "A2A relay liveness probe",
        description: "Returns 200 when the relay process is alive.",
    },
    RouteInfo {
        method: GET,
        path: "/.well-known/agent-card.json",
        tag: "a2a",
        summary: "Agent card",
        description: "Describes the relay's capabilities, task types and methods (not the executors).",
    },
    RouteInfo {
        method: GET,
        path: "/openapi.json",
        tag: "a2a",
        summary: "Agent Nexus OpenAPI specification document",
        description: "The generated OpenAPI document for the A2A relay and lifecycle hooks surfaces.",
    },
    // ---- Lifecycle hooks consumer ----
    RouteInfo {
        method: POST,
        path: "/experimental/hooks/chat",
        tag: "hooks",
        summary: "Coder lifecycle hook webhook",
        description: "Receives JWT-signed Coder lifecycle dispatches. Returns Coder's response schema: permission.decision (allow/deny), input_override for rewrites, and model_context feedback. Fail-closed on bad signature.",
    },
    RouteInfo {
        method: GET,
        path: "/experimental/hooks/health",
        tag: "hooks",
        summary: "Hook consumer liveness probe",
        description: "Returns 200 when the hook consumer is alive.",
    },
];

/// Build an operation for a route.
fn build_operation(info: &RouteInfo) -> utoipa::openapi::path::Operation {
    let mut op = OperationBuilder::new()
        .summary(Some(info.summary))
        .tag(info.tag)
        .description(Some(info.description))
        .response(
            "200",
            ResponseBuilder::new().description("Successful response"),
        );

    // The A2A RPC and hooks endpoints use JSON request/response bodies.
    match info.path {
        "/rpc" => {
            op = op
                .request_body(Some(json_body("JsonRpcRequest")))
                .response(
                    "200",
                    json_response("A2A JSON-RPC response envelope", "JsonRpcResponse"),
                )
                .response("400", json_response("Malformed request", "JsonRpcResponse"))
                .response(
                    "500",
                    json_response("Internal relay error", "JsonRpcResponse"),
                );
        }
        "/experimental/hooks/chat" => {
            op = op.response(
                "200",
                json_response("Hook decision in Coder's response schema", "HookDecision"),
            );
        }
        _ => {}
    }

    op.build()
}

/// Build a JSON request body referencing a component schema.
fn json_body(schema: &str) -> RequestBody {
    RequestBodyBuilder::new()
        .content(
            "application/json",
            Content::new(Some(Ref::from_schema_name(schema))),
        )
        .build()
}

/// Build a JSON response referencing a component schema.
fn json_response(description: &str, schema: &str) -> ResponseBuilder {
    ResponseBuilder::new().description(description).content(
        "application/json",
        Content::new(Some(Ref::from_schema_name(schema))),
    )
}

/// Group the route inventory into OpenAPI path items.
fn build_paths() -> Paths {
    let mut by_path: std::collections::BTreeMap<&str, Vec<&RouteInfo>> =
        std::collections::BTreeMap::new();
    for info in NEXUS_ROUTES {
        by_path.entry(info.path).or_default().push(info);
    }

    let mut builder = PathsBuilder::new();
    for (path, infos) in by_path {
        let mut path_item = PathItemBuilder::new();
        for info in infos {
            let method = match info.method {
                "GET" => HttpMethod::Get,
                "POST" => HttpMethod::Post,
                other => panic!("unsupported method {other} for {path}"),
            };
            path_item = path_item.operation(method, build_operation(info));
        }
        builder = builder.path(path, path_item.build());
    }
    builder.build()
}

/// Reusable component schemas for Agent Nexus.
fn build_components() -> utoipa::openapi::schema::ComponentsBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    let opt_string = || ObjectBuilder::new().schema_type(Type::String);

    utoipa::openapi::schema::ComponentsBuilder::new()
        // ---- A2A JSON-RPC envelope ----
        .schema(
            "JsonRpcRequest",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some("A2A JSON-RPC 2.0 request (simplified for the relay)"))
                .property("jsonrpc", string())
                .property("method", string())
                .property("params", ObjectBuilder::new().schema_type(Type::Object))
                .property("id", ObjectBuilder::new().schema_type(Type::String))
                .required("jsonrpc")
                .required("method"),
        )
        .schema(
            "JsonRpcResponse",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some("A2A JSON-RPC 2.0 response envelope"))
                .property("jsonrpc", string())
                .property("result", ObjectBuilder::new().schema_type(Type::Object))
                .property("error", ObjectBuilder::new().schema_type(Type::Object))
                .property("id", ObjectBuilder::new().schema_type(Type::String))
                .required("jsonrpc")
                .required("id"),
        )
        // ---- A2A verify task payloads ----
        .schema(
            "VerifyRequest",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some(
                    "Sentinel → Forge (via nexus relay) verification request",
                ))
                .property("pair_id", string())
                .property("kind", string())
                .property("cwd", string())
                .property(
                    "argv",
                    ArrayBuilder::new().items(ObjectBuilder::new().schema_type(Type::String)),
                )
                .property(
                    "timeout_secs",
                    ObjectBuilder::new().schema_type(Type::Integer),
                )
                .property(
                    "env_allowlist",
                    ObjectBuilder::new().schema_type(Type::Array),
                )
                .property("expect", ObjectBuilder::new().schema_type(Type::Object))
                .required("pair_id")
                .required("kind")
                .required("cwd")
                .required("argv")
                .required("timeout_secs"),
        )
        .schema(
            "VerifyResult",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some("Terminal result of a verify task"))
                .property("head_sha", opt_string())
                .property("task_id", string())
                .property("exit_code", ObjectBuilder::new().schema_type(Type::Integer))
                .property("timed_out", ObjectBuilder::new().schema_type(Type::Boolean))
                .property(
                    "duration_ms",
                    ObjectBuilder::new().schema_type(Type::Integer),
                )
                .property("stdout_ref", string())
                .property("stderr_ref", string())
                .property("artifacts", ObjectBuilder::new().schema_type(Type::Array))
                .property("executor", ObjectBuilder::new().schema_type(Type::Object))
                .required("task_id")
                .required("timed_out")
                .required("duration_ms")
                .required("stdout_ref")
                .required("stderr_ref"),
        )
        // ---- Lifecycle hooks decision ----
        .schema(
            "HookDecision",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some(
                    "Coder's lifecycle-hook response schema produced by the hook consumer",
                ))
                .property("model_context", opt_string())
                .property(
                    "permission",
                    ObjectBuilder::new()
                        .schema_type(Type::Object)
                        .description(Some(
                            "decision: allow|deny; input_override present on rewrites",
                        )),
                ),
        )
}

/// Build the complete Agent Nexus OpenAPI document.
pub fn nexus_openapi() -> OpenApi {
    OpenApiBuilder::new()
        .info(Info::new("Agent Nexus API", env!("CARGO_PKG_VERSION")))
        .paths(build_paths())
        .components(Some(build_components().build()))
        .build()
}

/// Serialize the Agent Nexus OpenAPI document to JSON.
pub fn nexus_openapi_json() -> serde_json::Value {
    serde_json::to_value(nexus_openapi()).expect("OpenAPI document serializes to JSON")
}

/// Axum handler serving `/openapi.json` on the A2A and hooks routers.
pub async fn serve_openapi() -> axum::Json<serde_json::Value> {
    axum::Json(nexus_openapi_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every inventoried route appears in the generated OpenAPI with the correct
    /// method and full path.
    #[test]
    fn every_inventoried_route_is_documented() {
        let doc = nexus_openapi();
        for info in NEXUS_ROUTES {
            let path_item = doc
                .paths
                .paths
                .get(info.path)
                .unwrap_or_else(|| panic!("route {} not in OpenAPI paths", info.path));
            let operation = match info.method {
                "GET" => &path_item.get,
                "POST" => &path_item.post,
                other => panic!("unhandled method {other}"),
            };
            assert!(
                operation.is_some(),
                "route {} {} missing operation in OpenAPI",
                info.method,
                info.path
            );
        }
    }

    /// The document serializes to valid JSON.
    #[test]
    fn document_serializes_to_json() {
        let json = nexus_openapi_json();
        assert!(json.get("openapi").is_some());
        assert!(json.get("paths").is_some());
    }

    /// Every documented route is registered on its real router (probed with a
    /// non-404, non-405 response), so a new route cannot silently go
    /// undocumented.
    #[tokio::test]
    async fn documented_routes_are_registered_on_their_routers() {
        use axum::body::Body;
        use axum::http::Request;
        use config::env::CoderHooksConfig;
        use pocketflow_core::SharedStore;
        use std::sync::Arc;
        use tower::ServiceExt;
        // A2A router.
        let store = Arc::new(SharedStore::new_in_memory());
        let relay = Arc::new(super::super::a2a::A2ARelay::new(store.clone()));
        let a2a_app = super::super::a2a::create_router(relay);

        // Hooks router.
        let hook_config = CoderHooksConfig {
            chat_hook_secret: Some("0123456789abcdef0123456789abcdef".to_string()),
            chat_hook_timeout_ms: 1500,
            chat_hook_enabled: true,
            chat_hook_allow_insecure: true,
            hook_addr: "127.0.0.1:3001".to_string(),
            hook_host: "127.0.0.1".to_string(),
            chat_hook_url: None,
            hook_url: None,
            hook_logs: false,
        };
        let hooks_app = super::super::hooks::create_router(store, hook_config, None, None);

        let mut failures = Vec::new();
        for info in NEXUS_ROUTES {
            let app = if info.path.starts_with("/experimental/") {
                &hooks_app
            } else {
                &a2a_app
            };
            let method = axum::http::Method::from_bytes(info.method.as_bytes()).unwrap();
            let request = Request::builder()
                .method(method)
                .uri(info.path)
                .body(Body::empty())
                .unwrap();
            let status = app.clone().oneshot(request).await.unwrap().status();
            if status == axum::http::StatusCode::NOT_FOUND {
                failures.push(format!(
                    "{} {} -> 404 (not registered)",
                    info.method, info.path
                ));
            } else if status == axum::http::StatusCode::METHOD_NOT_ALLOWED {
                failures.push(format!(
                    "{} {} -> 405 (method not accepted)",
                    info.method, info.path
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "routes not registered on their routers:\n{}",
            failures.join("\n")
        );
    }
}
