//! OpenAPI (OpenAPI 3.1) documentation for the OpenFlows Manager HTTP API.
//!
//! The spec is generated with [`utoipa`] from the **same route table** used to
//! build the axum router (`crate::routes`), so a route cannot be registered
//! without being documented and vice versa. See
//! `docs/architecture/openapi-reference.md`.

use utoipa::openapi::{
    info::Info,
    path::{HttpMethod, OperationBuilder, Parameter, ParameterIn, PathItemBuilder, PathsBuilder},
    schema::SchemaType,
    Content, ObjectBuilder, OpenApi, OpenApiBuilder, Paths, Ref, RefOr, ResponseBuilder, Type,
};

/// Authentication/visibility model of a route, used to attach the correct
/// OpenAPI `security` requirement and descriptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// No authentication.
    None,
    /// Bearer token (human session or workspace runtime credential).
    Bearer,
    /// Browser session cookie.
    Session,
    /// Signed webhook (GitHub signature or Coder hook JWT).
    Webhook,
}

/// Flattened documentation metadata for one (method, path) operation, derived
/// from the shared route table.
#[derive(Debug, Clone)]
pub(crate) struct RouteMeta {
    pub method: &'static str,
    pub path: String,
    pub tag: &'static str,
    pub summary: &'static str,
    pub auth: Auth,
    pub is_json: bool,
    pub success: u16,
    pub description: Option<&'static str>,
}

/// Derive the full list of documented operations from the shared route table.
/// API routes are prefixed with `/api/v1`; the `/openapi.json` endpoint served
/// by the Swagger UI is added explicitly.
pub(crate) fn manager_routes() -> Vec<RouteMeta> {
    let mut out = Vec::new();
    for rt in crate::routes::root_routes() {
        for m in rt.methods {
            out.push(RouteMeta {
                method: m,
                path: rt.path.to_string(),
                tag: rt.tag,
                summary: rt.summary,
                auth: rt.auth,
                is_json: rt.is_json,
                success: rt.success,
                description: rt.description,
            });
        }
    }
    for rt in crate::routes::api_v1_routes() {
        // The nested `/` route is registered at `/api/v1` (no trailing slash).
        let full_path = if rt.path == "/" {
            "/api/v1".to_string()
        } else {
            format!("/api/v1{}", rt.path)
        };
        for m in rt.methods {
            out.push(RouteMeta {
                method: m,
                path: full_path.clone(),
                tag: rt.tag,
                summary: rt.summary,
                auth: rt.auth,
                is_json: rt.is_json,
                success: rt.success,
                description: rt.description,
            });
        }
    }
    // The spec endpoint itself (served by the Swagger UI merge).
    out.push(RouteMeta {
        method: "GET",
        path: "/openapi.json".to_string(),
        tag: "root",
        summary: "OpenAPI specification document",
        auth: Auth::None,
        is_json: true,
        success: 200,
        description: None,
    });
    out
}

/// Extract path parameter names from a templated path, e.g. `{org}` from
/// `/organizations/{org}`.
fn path_parameters(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    for segment in path.split('/') {
        if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            names.push(name.to_string());
        }
    }
    names
}

/// The OpenAPI security requirement for a route's auth model.
fn security_requirement(auth: Auth) -> Option<utoipa::openapi::SecurityRequirement> {
    match auth {
        Auth::Bearer => Some(utoipa::openapi::SecurityRequirement::new(
            "bearerAuth",
            [] as [&str; 0],
        )),
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

/// Build an [`Operation`] for one (method, path) route.
fn build_operation(meta: &RouteMeta) -> utoipa::openapi::path::Operation {
    let success = if meta.success == 204 {
        ResponseBuilder::new().description("No content")
    } else {
        ResponseBuilder::new().description("Successful response")
    };

    let mut op = OperationBuilder::new()
        .summary(Some(meta.summary))
        .tag(meta.tag)
        .response(meta.success.to_string(), success);

    if let Some(desc) = meta.description {
        op = op.description(Some(desc));
    }

    // Declare required path parameters for templated paths.
    for name in path_parameters(&meta.path) {
        let mut param = Parameter::new(name);
        param.parameter_in = ParameterIn::Path;
        param.schema = Some(RefOr::T(utoipa::openapi::Schema::Object(
            ObjectBuilder::new().schema_type(Type::String).build(),
        )));
        op = op.parameter(param);
    }

    if let Some(sec) = security_requirement(meta.auth) {
        op = op.security(sec);
    }

    if meta.is_json {
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

/// Group the route metadata into OpenAPI path items (one per path, all methods).
fn build_paths() -> Paths {
    let routes = manager_routes();
    let mut by_path: std::collections::BTreeMap<&str, Vec<&RouteMeta>> =
        std::collections::BTreeMap::new();
    for meta in &routes {
        by_path.entry(meta.path.as_str()).or_default().push(meta);
    }

    let mut builder = PathsBuilder::new();
    for (path, metas) in by_path {
        let mut path_item = PathItemBuilder::new();
        for meta in metas {
            path_item = path_item.operation(http_method(meta.method), build_operation(meta));
        }
        builder = builder.path(path, path_item.build());
    }
    builder.build()
}

/// Reusable component schemas + security schemes for the Manager API.
fn build_components() -> utoipa::openapi::schema::ComponentsBuilder {
    utoipa::openapi::schema::ComponentsBuilder::new()
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
            utoipa::openapi::security::SecurityScheme::Http(utoipa::openapi::security::Http::new(
                utoipa::openapi::security::HttpAuthScheme::Bearer,
            )),
        )
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

/// A schema that is a `string` OR `null` (OAS 3.1 multi-type), matching fields
/// that serialize absent `Option` values as explicit `null`.
fn nullable_string() -> ObjectBuilder {
    ObjectBuilder::new().schema_type(SchemaType::Array(vec![Type::String, Type::Null]))
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
        .property("coder_organization_id", nullable_string())
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
        // `github_repository_id` is the numeric `GithubId` wrapper.
        .property(
            "github_repository_id",
            ObjectBuilder::new().schema_type(Type::Integer),
        )
        .property("desired_state", string())
        .property("observed_state", string())
        .property(
            "fleet_size",
            ObjectBuilder::new().schema_type(Type::Integer),
        )
        .property("error_code", nullable_string())
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
        .property("current_step", nullable_string())
        .property(
            "attempt_count",
            ObjectBuilder::new().schema_type(Type::Integer),
        )
        .property("error_code", nullable_string())
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

    /// Every route in the shared table appears in the generated OpenAPI
    /// document with the correct HTTP method and full path.
    #[test]
    fn every_inventoried_route_is_documented() {
        let doc = manager_openapi();
        let paths = &doc.paths.paths;
        assert!(!paths.is_empty(), "OpenAPI document must contain paths");

        for meta in manager_routes() {
            let path_item = paths
                .get(meta.path.as_str())
                .unwrap_or_else(|| panic!("route {} not in OpenAPI paths", meta.path));
            let operation = match meta.method {
                "GET" => &path_item.get,
                "POST" => &path_item.post,
                "PATCH" => &path_item.patch,
                "DELETE" => &path_item.delete,
                other => panic!("unhandled method {other} for {}", meta.path),
            };
            assert!(
                operation.is_some(),
                "route {} {} missing operation in OpenAPI",
                meta.method,
                meta.path
            );
        }
    }

    /// Templated paths declare their required path parameters.
    #[test]
    fn templated_paths_declare_path_parameters() {
        let doc = manager_openapi();
        let paths = &doc.paths.paths;
        for meta in manager_routes() {
            let names = path_parameters(&meta.path);
            if names.is_empty() {
                continue;
            }
            let path_item = paths.get(meta.path.as_str()).unwrap();
            let operation = match meta.method {
                "GET" => path_item.get.as_ref(),
                "POST" => path_item.post.as_ref(),
                "PATCH" => path_item.patch.as_ref(),
                "DELETE" => path_item.delete.as_ref(),
                _ => None,
            }
            .expect("operation exists");
            let declared = operation
                .parameters
                .as_ref()
                .map(|ps| {
                    ps.iter()
                        .filter_map(|p| match p {
                            RefOr::T(param) => Some(param),
                            RefOr::Ref(_) => None,
                        })
                        .filter(|p| matches!(p.parameter_in, ParameterIn::Path))
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for name in &names {
                assert!(
                    declared.contains(name),
                    "path parameter {name} not declared for {} {}",
                    meta.method,
                    meta.path
                );
            }
        }
    }

    /// The document serializes to valid JSON.
    #[test]
    fn document_serializes_to_json() {
        let json = manager_openapi_json();
        assert!(json.get("openapi").is_some());
        assert!(json.get("paths").is_some());
        assert!(json.get("components").is_some());
    }

    /// Every route in the shared table is actually registered on the router
    /// (probed with a non-404, non-405 response).
    #[tokio::test]
    async fn documented_routes_are_registered_on_the_router() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = crate::server::create_router(crate::server::AppState::for_tests());

        let mut failures = Vec::new();
        for meta in manager_routes() {
            let method = axum::http::Method::from_bytes(meta.method.as_bytes()).unwrap();
            let request = Request::builder()
                .method(method)
                .uri(meta.path.as_str())
                .body(Body::empty())
                .unwrap();
            let status = app.clone().oneshot(request).await.unwrap().status();
            if status == axum::http::StatusCode::NOT_FOUND {
                failures.push(format!(
                    "{} {} -> 404 (not registered)",
                    meta.method, meta.path
                ));
            } else if status == axum::http::StatusCode::METHOD_NOT_ALLOWED {
                failures.push(format!(
                    "{} {} -> 405 (method not accepted)",
                    meta.method, meta.path
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

        let swagger_csp = csp_of(app.clone(), "/swagger-ui/").await;
        assert!(
            swagger_csp.contains("script-src 'self'"),
            "swagger-ui CSP must allow same-origin scripts, got: {swagger_csp}"
        );
        assert!(
            swagger_csp.contains("connect-src 'self'"),
            "swagger-ui CSP must allow same-origin fetches, got: {swagger_csp}"
        );

        for path in ["/health", "/api/v1/me", "/"] {
            let csp = csp_of(app.clone(), path).await;
            assert!(
                csp.starts_with("default-src 'none'"),
                "route {path} must keep the strict CSP, got: {csp}"
            );
        }
    }
}
