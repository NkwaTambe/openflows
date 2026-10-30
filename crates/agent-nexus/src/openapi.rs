//! OpenAPI (OpenAPI 3.1) documentation for Agent Nexus HTTP surfaces.
//!
//! Agent Nexus hosts two separate listeners with disjoint routes, so they get
//! **filtered per-listener documents** rather than a single combined one:
//!
//! - **A2A relay** (`crates/agent-nexus/src/a2a/http_server.rs`): JSON-RPC
//!   (`POST /rpc`), an SSE stream (`GET /`), health, and an agent card.
//! - **Lifecycle hooks consumer** (`crates/agent-nexus/src/hooks/server.rs`):
//!   a JWT-signed webhook and its health probe.
//!
//! Each listener serves only the routes it actually registers, so advertised
//! calls never 404 on the serving listener.

use utoipa::openapi::{
    info::Info,
    path::{HttpMethod, OperationBuilder, Parameter, ParameterIn, PathItemBuilder, PathsBuilder},
    request_body::{RequestBody, RequestBodyBuilder},
    schema::{ArrayBuilder, SchemaType},
    Content, ObjectBuilder, OpenApi, OpenApiBuilder, Paths, Ref, RefOr, ResponseBuilder, Schema,
    Type,
};

const GET: &str = "GET";
const POST: &str = "POST";

/// A single documented Agent Nexus route.
#[derive(Debug, Clone, Copy)]
pub struct RouteInfo {
    /// HTTP method, e.g. `"GET"`.
    pub method: &'static str,
    /// Full path as registered on the listener.
    pub path: &'static str,
    /// Functional tag (`a2a` or `hooks`).
    pub tag: &'static str,
    /// One-line human summary.
    pub summary: &'static str,
    /// Longer description (protocol details, methods, query parameters).
    pub description: &'static str,
}

/// Routes served by the A2A relay listener.
const A2A_ROUTES: &[RouteInfo] = &[
    RouteInfo {
        method: POST,
        path: "/rpc",
        tag: "a2a",
        summary: "A2A JSON-RPC dispatch",
        description: "Dispatches A2A methods by name in the JSON-RPC body: message/send, tasks/get, tasks/claim, tasks/complete, tasks/cancel, tasks/resubscribe, tasks/push_progress. JSON-RPC errors are returned inside the 200 response envelope, not as separate HTTP error bodies.",
    },
    RouteInfo {
        method: GET,
        path: "/",
        tag: "a2a",
        summary: "SSE verify-task progress stream",
        description: "Server-Sent Events stream of VerifyProgressEvent chunks. Requires a task_id query parameter; replays buffered events on connect, then streams live until completion or disconnect.",
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
        summary: "Agent Nexus A2A OpenAPI specification",
        description: "The generated OpenAPI document for the A2A relay listener.",
    },
];

/// Routes served by the lifecycle hooks consumer listener.
const HOOKS_ROUTES: &[RouteInfo] = &[
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
    RouteInfo {
        method: GET,
        path: "/openapi.json",
        tag: "hooks",
        summary: "Agent Nexus hooks OpenAPI specification",
        description: "The generated OpenAPI document for the lifecycle hooks listener.",
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

    match info.path {
        "/rpc" => {
            op = op.request_body(Some(json_body("JsonRpcRequest"))).response(
                "200",
                json_response(
                    "JSON-RPC response (carries either result or error)",
                    "JsonRpcResponse",
                ),
            );
            // Malformed JSON bodies are rejected by axum before the handler and
            // do not use the JSON-RPC envelope, so no 400/500 are documented here.
        }
        "/" => {
            // The SSE stream requires a task_id query parameter.
            let mut param = Parameter::new("task_id");
            param.parameter_in = ParameterIn::Query;
            param.schema = Some(RefOr::T(Schema::Object(
                ObjectBuilder::new().schema_type(Type::String).build(),
            )));
            op = op
                .parameter(param)
                .response(
                    "400",
                    ResponseBuilder::new().description("Missing or empty task_id query parameter"),
                )
                .response(
                    "200",
                    ResponseBuilder::new()
                        .description("Server-Sent Events stream of VerifyProgressEvent chunks")
                        .content(
                            "text/event-stream",
                            Content::new(Some(RefOr::T(Schema::Object(
                                ObjectBuilder::new().schema_type(Type::String).build(),
                            )))),
                        ),
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

/// Group a route list into OpenAPI path items.
fn build_paths(routes: &[RouteInfo]) -> Paths {
    let mut by_path: std::collections::BTreeMap<&str, Vec<&RouteInfo>> =
        std::collections::BTreeMap::new();
    for info in routes {
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

/// A schema that is `string` OR `null` (OAS 3.1 multi-type).
fn nullable_string() -> ObjectBuilder {
    ObjectBuilder::new().schema_type(SchemaType::Array(vec![Type::String, Type::Null]))
}

/// Build the OpenAPI document for a given route list.
fn build_openapi(routes: &[RouteInfo], title: &str) -> OpenApi {
    OpenApiBuilder::new()
        .info(Info::new(title, env!("CARGO_PKG_VERSION")))
        .paths(build_paths(routes))
        .components(Some(components()))
        .build()
}

/// Reusable component schemas for Agent Nexus.
fn components() -> utoipa::openapi::schema::Components {
    let string = || ObjectBuilder::new().schema_type(Type::String);

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
                // The relay accepts and echoes numeric or string ids.
                .property(
                    "id",
                    ObjectBuilder::new()
                        .schema_type(SchemaType::Array(vec![Type::String, Type::Integer])),
                )
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
                .property(
                    "id",
                    ObjectBuilder::new()
                        .schema_type(SchemaType::Array(vec![Type::String, Type::Integer])),
                )
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
                    ArrayBuilder::new().items(ObjectBuilder::new().schema_type(Type::String)),
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
                .property("head_sha", nullable_string())
                .property("task_id", string())
                .property(
                    "exit_code",
                    ObjectBuilder::new()
                        .schema_type(SchemaType::Array(vec![Type::Integer, Type::Null])),
                )
                .property("timed_out", ObjectBuilder::new().schema_type(Type::Boolean))
                .property(
                    "duration_ms",
                    ObjectBuilder::new().schema_type(Type::Integer),
                )
                .property("stdout_ref", string())
                .property("stderr_ref", string())
                .property(
                    "artifacts",
                    ArrayBuilder::new().items(ObjectBuilder::new().schema_type(Type::Object)),
                )
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
                .property("model_context", nullable_string())
                .property(
                    "permission",
                    ObjectBuilder::new()
                        .schema_type(Type::Object)
                        .description(Some(
                            "decision: allow|deny; input_override present on rewrites",
                        )),
                ),
        )
        .build()
}

/// Build the A2A relay OpenAPI document.
pub fn a2a_openapi() -> OpenApi {
    build_openapi(A2A_ROUTES, "Agent Nexus A2A Relay API")
}

/// Build the lifecycle hooks OpenAPI document.
pub fn hooks_openapi() -> OpenApi {
    build_openapi(HOOKS_ROUTES, "Agent Nexus Lifecycle Hooks API")
}

fn to_json(api: OpenApi) -> serde_json::Value {
    serde_json::to_value(api).expect("OpenAPI document serializes to JSON")
}

/// Serialize the A2A OpenAPI document to JSON.
pub fn a2a_openapi_json() -> serde_json::Value {
    to_json(a2a_openapi())
}

/// Serialize the hooks OpenAPI document to JSON.
pub fn hooks_openapi_json() -> serde_json::Value {
    to_json(hooks_openapi())
}

/// Axum handler serving the A2A document at `/openapi.json`.
pub async fn serve_a2a_openapi() -> axum::Json<serde_json::Value> {
    axum::Json(a2a_openapi_json())
}

/// Axum handler serving the hooks document at `/openapi.json`.
pub async fn serve_hooks_openapi() -> axum::Json<serde_json::Value> {
    axum::Json(hooks_openapi_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every A2A route appears in the A2A document with the correct method/path.
    #[test]
    fn every_a2a_route_is_documented() {
        let doc = a2a_openapi();
        for info in A2A_ROUTES {
            assert_operation(&doc, info);
        }
    }

    /// Every hooks route appears in the hooks document.
    #[test]
    fn every_hooks_route_is_documented() {
        let doc = hooks_openapi();
        for info in HOOKS_ROUTES {
            assert_operation(&doc, info);
        }
    }

    fn assert_operation(doc: &OpenApi, info: &RouteInfo) {
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

    /// Documents serialize to valid JSON.
    #[test]
    fn documents_serialize_to_json() {
        for json in [a2a_openapi_json(), hooks_openapi_json()] {
            assert!(json.get("openapi").is_some());
            assert!(json.get("paths").is_some());
        }
    }

    /// Each listener only serves its own routes (no cross-advertising), and
    /// every documented route is registered on its real router.
    #[tokio::test]
    async fn documented_routes_are_registered_on_their_listeners() {
        use axum::body::Body;
        use axum::http::Request;
        use config::env::CoderHooksConfig;
        use pocketflow_core::SharedStore;
        use std::sync::Arc;
        use tower::ServiceExt;

        let store = Arc::new(SharedStore::new_in_memory());
        let relay = Arc::new(super::super::a2a::A2ARelay::new(store.clone()));
        let a2a_app = super::super::a2a::create_router(relay);

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
        for (routes, app) in [(A2A_ROUTES, &a2a_app), (HOOKS_ROUTES, &hooks_app)] {
            for info in routes {
                let method = axum::http::Method::from_bytes(info.method.as_bytes()).unwrap();
                let request = Request::builder()
                    .method(method)
                    .uri(info.path)
                    .body(Body::empty())
                    .unwrap();
                let status = app.clone().oneshot(request).await.unwrap().status();
                if status == axum::http::StatusCode::NOT_FOUND {
                    failures.push(format!("{} {} -> 404", info.method, info.path));
                } else if status == axum::http::StatusCode::METHOD_NOT_ALLOWED {
                    failures.push(format!("{} {} -> 405", info.method, info.path));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "routes not registered on their listeners:\n{}",
            failures.join("\n")
        );
    }

    /// The A2A document's SSE operation declares the required task_id query
    /// parameter and a text/event-stream response.
    #[test]
    fn stream_operation_declares_task_id_query_param() {
        let doc = a2a_openapi();
        let path_item = doc.paths.paths.get("/").expect("/ path");
        let op = path_item.get.as_ref().expect("GET / operation");
        let params = op.parameters.as_ref().expect("parameters present");
        let has_task_id = params.iter().any(|p| match p {
            RefOr::T(parameter) => {
                parameter.name == "task_id"
                    && matches!(parameter.parameter_in, ParameterIn::Query)
                    && matches!(parameter.required, utoipa::openapi::Required::True)
            }
            RefOr::Ref(_) => false,
        });
        assert!(
            has_task_id,
            "GET / must declare a required task_id query parameter"
        );
    }
}
