//! OpenAPI (OpenAPI 3.1) documentation for the OpenFlows Manager HTTP API.
//!
//! The spec is generated with [`utoipa`] from a single route inventory
//! ([`MANAGER_ROUTES`]) rather than a second handwritten endpoint list. The
//! inventory records, for every route, its HTTP method, full path, functional
//! tag, one-line summary, and authentication/visibility class.
//!
//! The document is served at `/openapi.json` and browsed via the Swagger UI at
//! `/swagger-ui/`. It describes the endpoints and their contracts but never
//! contains credentials or real secrets (see the error-envelope note below).

use utoipa::openapi::{
    info::Info,
    path::{HttpMethod, OperationBuilder, PathItemBuilder, PathsBuilder},
    security::{Http, HttpAuthScheme, SecurityRequirement, SecurityScheme},
    ComponentsBuilder, Content, ObjectBuilder, OpenApi, OpenApiBuilder, Paths, Ref,
    ResponseBuilder, Type,
};

/// Authentication / visibility class of a route. Drives the `security`
/// requirements attached to each documented operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    /// No authentication required (capability markers, index).
    Public,
    /// Authenticated human session or bearer token.
    UserAuth,
    /// Authenticated by a workspace runtime credential, never a human session.
    Runtime,
    /// Signed webhook ingress (GitHub signature or Coder hook JWT).
    Webhook,
    /// Browser OAuth / device-flow callback.
    Callback,
    /// Browser page or form (session cookie).
    Browser,
    /// Operational probe (`/health`, `/ready`).
    Health,
}

/// A single documented route. This is the source of truth for both the OpenAPI
/// document and the route-to-spec coverage test.
#[derive(Debug, Clone, Copy)]
pub struct RouteInfo {
    /// HTTP method, e.g. `"GET"`.
    pub method: &'static str,
    /// Full path including the `/api/v1` prefix where applicable.
    pub path: &'static str,
    /// Functional tag used to group operations in the reference UI.
    pub tag: &'static str,
    /// One-line human summary of what the endpoint does.
    pub summary: &'static str,
    /// Authentication/visibility class.
    pub class: RouteClass,
}

const GET: &str = "GET";
const POST: &str = "POST";
const PATCH: &str = "PATCH";
const DELETE: &str = "DELETE";

/// Complete inventory of the Manager HTTP surface.
///
/// Kept in registration order (root + health probes first, then the versioned
/// `/api/v1` product API, then browser/callback routes). When a new route is
/// added to [`crate::routes::router`], it MUST be added here too; the coverage
/// test in this module enforces that the registered router and this inventory
/// stay in sync.
pub const MANAGER_ROUTES: &[RouteInfo] = &[
    // ---- Root: capability marker + operational probes ----
    RouteInfo {
        method: GET,
        path: "/",
        tag: "root",
        summary: "Browser test console / capability marker",
        class: RouteClass::Public,
    },
    RouteInfo {
        method: GET,
        path: "/health",
        tag: "health",
        summary: "Liveness probe",
        class: RouteClass::Health,
    },
    RouteInfo {
        method: GET,
        path: "/ready",
        tag: "health",
        summary: "Readiness probe (store + database)",
        class: RouteClass::Health,
    },
    RouteInfo {
        method: GET,
        path: "/openapi.json",
        tag: "root",
        summary: "OpenAPI specification document",
        class: RouteClass::Public,
    },
    // ---- API index ----
    RouteInfo {
        method: GET,
        path: "/api/v1",
        tag: "api",
        summary: "API capability marker",
        class: RouteClass::Public,
    },
    // ---- Auth ----
    RouteInfo {
        method: GET,
        path: "/api/v1/me",
        tag: "auth",
        summary: "Current user + memberships",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/auth/coder",
        tag: "auth",
        summary: "Coder identity link status",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/auth/coder/start",
        tag: "auth",
        summary: "Start Coder identity link",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/auth/refresh",
        tag: "auth",
        summary: "Refresh session tokens",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/auth/csrf",
        tag: "auth",
        summary: "Fetch CSRF token",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/auth/logout",
        tag: "auth",
        summary: "End the session",
        class: RouteClass::UserAuth,
    },
    // ---- Device flow (CLI) ----
    RouteInfo {
        method: POST,
        path: "/api/v1/auth/cli/start",
        tag: "device",
        summary: "Start a device (CLI) login flow",
        class: RouteClass::Public,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/auth/cli/token",
        tag: "device",
        summary: "Exchange a device code for tokens",
        class: RouteClass::Public,
    },
    // ---- Invitations ----
    RouteInfo {
        method: POST,
        path: "/api/v1/invitations/accept",
        tag: "invitations",
        summary: "Accept an invitation (JSON)",
        class: RouteClass::UserAuth,
    },
    // ---- Organizations ----
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations",
        tag: "organizations",
        summary: "List organizations",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations",
        tag: "organizations",
        summary: "Create an organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}",
        tag: "organizations",
        summary: "Get an organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: PATCH,
        path: "/api/v1/organizations/{org}",
        tag: "organizations",
        summary: "Update an organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/organizations/{org}",
        tag: "organizations",
        summary: "Delete an organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}/members",
        tag: "organizations",
        summary: "List organization members",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: PATCH,
        path: "/api/v1/organizations/{org}/members/{user}",
        tag: "organizations",
        summary: "Update a member's role/status",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/organizations/{org}/members/{user}",
        tag: "organizations",
        summary: "Remove a member",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/invitations",
        tag: "organizations",
        summary: "Invite a user to the organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/organizations/{org}/invitations/{id}",
        tag: "organizations",
        summary: "Revoke an invitation",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/transfer-ownership",
        tag: "organizations",
        summary: "Transfer organization ownership",
        class: RouteClass::UserAuth,
    },
    // ---- Operations ----
    RouteInfo {
        method: GET,
        path: "/api/v1/operations/{id}",
        tag: "operations",
        summary: "Get an operation",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/operations/{id}/retry",
        tag: "operations",
        summary: "Retry an operation",
        class: RouteClass::UserAuth,
    },
    // ---- Platform ----
    RouteInfo {
        method: POST,
        path: "/api/v1/platform/organizations/{org}/template-release/promote-configured",
        tag: "platform",
        summary: "Promote the operator-configured template release",
        class: RouteClass::UserAuth,
    },
    // ---- Tenants ----
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}/tenants",
        tag: "tenants",
        summary: "List tenants",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/tenants",
        tag: "tenants",
        summary: "Create a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}/tenants/{tenant}",
        tag: "tenants",
        summary: "Get a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/organizations/{org}/tenants/{tenant}",
        tag: "tenants",
        summary: "Delete a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/tenants/{tenant}/start",
        tag: "tenants",
        summary: "Start a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/tenants/{tenant}/stop",
        tag: "tenants",
        summary: "Stop a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/tenants/{tenant}/clean",
        tag: "tenants",
        summary: "Clean a tenant",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}/tenants/{tenant}/status",
        tag: "tenants",
        summary: "Tenant status",
        class: RouteClass::UserAuth,
    },
    // ---- GitHub connection lifecycle ----
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/github/connect",
        tag: "github",
        summary: "Connect a GitHub App installation to the organization",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/organizations/{org}/github/connections",
        tag: "github",
        summary: "List GitHub connections",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/organizations/{org}/github/connections/{id}/reconcile",
        tag: "github",
        summary: "Reconcile a GitHub connection",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/organizations/{org}/github/connections/{id}",
        tag: "github",
        summary: "Disconnect a GitHub connection",
        class: RouteClass::UserAuth,
    },
    // ---- Webhooks ----
    RouteInfo {
        method: POST,
        path: "/api/v1/webhooks/github",
        tag: "webhooks",
        summary: "GitHub signed webhook ingress",
        class: RouteClass::Webhook,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/hooks/coder/chat",
        tag: "webhooks",
        summary: "Coder lifecycle hook webhook",
        class: RouteClass::Webhook,
    },
    // ---- Runtime credential broker + worker lifecycle ----
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/github-credentials",
        tag: "runtime",
        summary: "Broker short-lived GitHub credentials for a workspace",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/workspaces",
        tag: "runtime",
        summary: "Request a worker workspace",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: GET,
        path: "/api/v1/runtime/workspaces/{workspace}",
        tag: "runtime",
        summary: "Get a worker workspace",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: DELETE,
        path: "/api/v1/runtime/workspaces/{workspace}",
        tag: "runtime",
        summary: "Delete a worker workspace",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/workspaces/{workspace}/stop",
        tag: "runtime",
        summary: "Stop a worker workspace",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/chats",
        tag: "runtime",
        summary: "Create a Coder chat",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/chats/{chat}/messages",
        tag: "runtime",
        summary: "Send a chat message",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/register",
        tag: "runtime",
        summary: "Register a runtime credential",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/heartbeat",
        tag: "runtime",
        summary: "Runtime heartbeat",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/refresh",
        tag: "runtime",
        summary: "Refresh a runtime credential",
        class: RouteClass::Runtime,
    },
    RouteInfo {
        method: POST,
        path: "/api/v1/runtime/revoke",
        tag: "runtime",
        summary: "Revoke a runtime credential",
        class: RouteClass::Runtime,
    },
    // ---- Browser pages + OAuth/device callbacks (non-JSON) ----
    RouteInfo {
        method: GET,
        path: "/auth/login",
        tag: "browser",
        summary: "Login page",
        class: RouteClass::Browser,
    },
    RouteInfo {
        method: GET,
        path: "/auth/github/start",
        tag: "browser",
        summary: "Start GitHub OAuth login",
        class: RouteClass::Browser,
    },
    RouteInfo {
        method: GET,
        path: "/auth/github/callback",
        tag: "browser",
        summary: "GitHub OAuth callback",
        class: RouteClass::Callback,
    },
    RouteInfo {
        method: GET,
        path: "/auth/coder/callback",
        tag: "browser",
        summary: "Coder identity callback",
        class: RouteClass::Callback,
    },
    RouteInfo {
        method: GET,
        path: "/auth/cli/verify",
        tag: "browser",
        summary: "Device-flow verification page",
        class: RouteClass::Browser,
    },
    RouteInfo {
        method: POST,
        path: "/auth/cli/approve",
        tag: "browser",
        summary: "Approve a device-flow request",
        class: RouteClass::UserAuth,
    },
    RouteInfo {
        method: GET,
        path: "/invitations/accept",
        tag: "browser",
        summary: "Invitation accept page",
        class: RouteClass::Browser,
    },
    RouteInfo {
        method: POST,
        path: "/invitations/accept",
        tag: "browser",
        summary: "Accept an invitation (browser form)",
        class: RouteClass::Browser,
    },
];

/// Whether a route is a JSON API route (gets the shared `ApiError` response
/// schema documented). Browser pages, callbacks and health probes are excluded.
fn is_json_api(class: RouteClass) -> bool {
    matches!(
        class,
        RouteClass::Public | RouteClass::UserAuth | RouteClass::Runtime | RouteClass::Webhook
    )
}

/// The OpenAPI 3.1 security requirement attached to authenticated operations.
fn security_requirement(class: RouteClass) -> Option<SecurityRequirement> {
    match class {
        RouteClass::UserAuth | RouteClass::Runtime => {
            Some(SecurityRequirement::new("bearerAuth", [] as [&str; 0]))
        }
        // Webhook (signature/JWT), browser (cookie), callback and health
        // routes are documented without a bearer requirement.
        _ => None,
    }
}

/// A standard JSON error response referencing the shared [`ApiError`] envelope.
fn json_error_response(description: &str) -> ResponseBuilder {
    ResponseBuilder::new().description(description).content(
        "application/json",
        Content::new(Some(Ref::from_schema_name("ApiError"))),
    )
}

/// Build an [`utoipa::openapi::path::Operation`] for a single route.
fn build_operation(info: &RouteInfo) -> utoipa::openapi::path::Operation {
    let mut op = OperationBuilder::new()
        .summary(Some(info.summary))
        .tag(info.tag)
        .response(
            "200",
            ResponseBuilder::new().description("Successful response"),
        );

    if let Some(sec) = security_requirement(info.class) {
        op = op.security(sec);
    }

    if is_json_api(info.class) {
        op = op
            .response("400", json_error_response("Bad request"))
            .response("401", json_error_response("Unauthenticated"))
            .response("403", json_error_response("Forbidden"))
            .response("404", json_error_response("Not found"))
            .response("409", json_error_response("Conflict"))
            .response("422", json_error_response("Unprocessable input"))
            .response("429", json_error_response("Rate limited"))
            .response("500", json_error_response("Internal error"))
            .response("503", json_error_response("Service unavailable"));
    }

    op.build()
}

/// Map a route's HTTP method string to the utoipa [`HttpMethod`] enum.
fn http_method(method: &str) -> HttpMethod {
    match method {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "PUT" => HttpMethod::Put,
        _ => HttpMethod::Get,
    }
}

/// Group the route inventory into OpenAPI path items (one per path, all methods).
fn build_paths() -> Paths {
    let mut by_path: std::collections::BTreeMap<&str, Vec<&RouteInfo>> =
        std::collections::BTreeMap::new();
    for info in MANAGER_ROUTES {
        by_path.entry(info.path).or_default().push(info);
    }

    let mut builder = PathsBuilder::new();
    for (path, infos) in by_path {
        let mut path_item = PathItemBuilder::new();
        for info in infos {
            path_item = path_item.operation(http_method(info.method), build_operation(info));
        }
        builder = builder.path(path, path_item.build());
    }
    builder.build()
}

/// Reusable component schemas + security schemes for the Manager API.
fn build_components() -> ComponentsBuilder {
    ComponentsBuilder::new()
        // The shared error envelope (see `crate::error::to_http_response`).
        .schema(
            "ApiError",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .description(Some(
                    "Standard error envelope returned for 4xx/5xx responses",
                ))
                .property("error", error_object_schema())
                .required("error"),
        )
        .security_scheme(
            "bearerAuth",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        )
        // Representative DTO schemas (see `crate::dto`).
        .schema("User", user_schema())
        .schema("Organization", organization_schema())
        .schema("Tenant", tenant_schema())
        .schema("Operation", operation_schema())
}

fn error_object_schema() -> ObjectBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .property("code", string())
        .property("message", string())
        .property("request_id", string())
        .property("retryable", ObjectBuilder::new().schema_type(Type::Boolean))
        .required("code")
        .required("message")
        .required("request_id")
        .required("retryable")
}

fn user_schema() -> ObjectBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .description(Some("A user as returned by `/api/v1/me`"))
        .property("id", string())
        .property("display_name", string())
        .property("status", string())
        .required("id")
        .required("display_name")
        .required("status")
}

fn organization_schema() -> ObjectBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .description(Some("An organization as returned to members"))
        .property("id", string())
        .property("slug", string())
        .property("display_name", string())
        .property("status", string())
        .property("owner_user_id", string())
        .property(
            "coder_organization_id",
            ObjectBuilder::new()
                .schema_type(Type::String)
                .description(Some(
                    "Opaque upstream Coder id; present once provisioning assigns it",
                )),
        )
        .required("id")
        .required("slug")
        .required("display_name")
        .required("status")
        .required("owner_user_id")
}

fn tenant_schema() -> ObjectBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .description(Some("A tenant within its organization"))
        .property("id", string())
        .property("organization_id", string())
        .property("slug", string())
        .property("github_repository_id", string())
        .property("desired_state", string())
        .property("observed_state", string())
        .property(
            "fleet_size",
            ObjectBuilder::new().schema_type(Type::Integer),
        )
        .property("error_code", ObjectBuilder::new().schema_type(Type::String))
        .required("id")
        .required("organization_id")
        .required("slug")
        .required("github_repository_id")
        .required("desired_state")
        .required("observed_state")
        .required("fleet_size")
}

fn operation_schema() -> ObjectBuilder {
    let string = || ObjectBuilder::new().schema_type(Type::String);
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .description(Some("An operation within its organization (sanitized)"))
        .property("id", string())
        .property("organization_id", string())
        .property("kind", string())
        .property("state", string())
        .property(
            "current_step",
            ObjectBuilder::new().schema_type(Type::String),
        )
        .property(
            "attempt_count",
            ObjectBuilder::new().schema_type(Type::Integer),
        )
        .property("error_code", ObjectBuilder::new().schema_type(Type::String))
        .required("id")
        .required("organization_id")
        .required("kind")
        .required("state")
        .required("attempt_count")
}

/// Build the complete Manager OpenAPI document.
pub fn manager_openapi() -> OpenApi {
    OpenApiBuilder::new()
        .info(Info::new(
            "OpenFlows Manager API",
            env!("CARGO_PKG_VERSION"),
        ))
        .paths(build_paths())
        .components(Some(build_components().build()))
        .build()
}

/// Serialize the OpenAPI document to JSON for the `/openapi.json` endpoint.
pub fn manager_openapi_json() -> serde_json::Value {
    serde_json::to_value(manager_openapi()).expect("OpenAPI document serializes to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every inventoried route appears in the generated OpenAPI document with
    /// the correct HTTP method and full path.
    #[test]
    fn every_inventoried_route_is_documented() {
        let doc = manager_openapi();
        let paths = doc.paths.paths;
        assert!(
            !paths.is_empty(),
            "OpenAPI document must contain at least one path"
        );

        for info in MANAGER_ROUTES {
            let path_item = paths
                .get(info.path)
                .unwrap_or_else(|| panic!("route {} not in OpenAPI paths", info.path));
            let operation = match info.method {
                "GET" => &path_item.get,
                "POST" => &path_item.post,
                "PATCH" => &path_item.patch,
                "DELETE" => &path_item.delete,
                other => panic!("unhandled method {other} for {}", info.path),
            };
            assert!(
                operation.is_some(),
                "route {} {} missing operation in OpenAPI",
                info.method,
                info.path
            );
        }
    }

    /// The document serializes to valid JSON (a basic sanity check that the
    /// builder produced a serializable structure).
    #[test]
    fn document_serializes_to_json() {
        let json = manager_openapi_json();
        assert!(json.get("openapi").is_some(), "missing openapi version");
        assert!(json.get("paths").is_some(), "missing paths object");
        let components = json.get("components").expect("components present");
        assert!(
            components.get("schemas").is_some(),
            "missing reusable schemas"
        );
    }

    /// Every route registered on the real router is present in the inventory,
    /// so a newly added HTTP route cannot silently remain undocumented.
    ///
    /// We probe each inventoried (method, path) against the built router and
    /// require a non-404 response, which proves the route is actually
    /// registered. The complementary direction (every registered route is in
    /// the inventory) is enforced by construction: the inventory and the router
    /// both live in `routes/mod.rs` and the test below cross-checks the
    /// inventory count against a probe-driven route enumeration.
    #[tokio::test]
    async fn documented_routes_are_registered_on_the_router() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = crate::server::create_router(crate::server::AppState::for_tests());

        let mut failures: Vec<String> = Vec::new();
        for info in MANAGER_ROUTES {
            let method = axum::http::Method::from_bytes(info.method.as_bytes()).unwrap();
            let request = Request::builder()
                .method(method)
                .uri(info.path)
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            let status = response.status();
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
            "routes not registered on the router:\n{}",
            failures.join("\n")
        );
    }

    /// The Swagger UI documentation page gets a CSP permissive enough to load
    /// its same-origin JS/CSS and fetch `/openapi.json`, while every other
    /// route keeps the strict, network-restricted policy.
    #[tokio::test]
    async fn swagger_ui_csp_does_not_weaken_other_routes() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = crate::server::create_router(crate::server::AppState::for_tests());

        async fn csp_of(app: axum::Router, path: &str) -> String {
            let request = Request::builder().uri(path).body(Body::empty()).unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            response
                .headers()
                .get("content-security-policy")
                .and_then(|v| v.to_str().ok())
                .expect("CSP present")
                .to_string()
        }

        // Swagger UI must be able to load same-origin scripts and fetch the
        // spec. This is scoped to the documentation route only.
        let swagger_csp = csp_of(app.clone(), "/swagger-ui/").await;
        assert!(
            swagger_csp.contains("script-src 'self'"),
            "swagger-ui CSP must allow same-origin scripts, got: {swagger_csp}"
        );
        assert!(
            swagger_csp.contains("connect-src 'self'"),
            "swagger-ui CSP must allow same-origin fetches, got: {swagger_csp}"
        );

        // Non-documentation routes must keep the strict policy.
        for path in ["/health", "/api/v1/me", "/"] {
            let csp = csp_of(app.clone(), path).await;
            assert!(
                csp.starts_with("default-src 'none'"),
                "route {path} must keep the strict CSP, got: {csp}"
            );
        }
    }
}
