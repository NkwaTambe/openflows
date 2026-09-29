//! Server bootstrap, application state, and graceful serving utilities.

use crate::error::ManagerError;
use axum::{routing::get, Router};
use pocketflow_core::SharedStore;
use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc};
use tokio::{
    net::TcpListener,
    time::{timeout, Duration},
};

const READINESS_CHECK_TIMEOUT: Duration = Duration::from_secs(1);

/// Token used by the in-memory test state. Tests authenticate with this value
/// so auth is exercised end-to-end without depending on the environment.
pub const TEST_AUTH_TOKEN: &str = "test-token";

/// Async dependency probe used by `/ready`.
///
/// The trait keeps the readiness endpoint decoupled from Redis and gives tests
/// a small seam for simulating unavailable or stalled dependencies.
pub trait ReadinessCheck: Send + Sync {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<(), ManagerError>> + Send + '_>>;
}

#[derive(Clone)]
struct StoreReadiness {
    // SharedStore is cloned cheaply and owns the backing connection state.
    // Keeping the concrete store behind this private adapter lets AppState
    // expose only the trait object used by handlers.
    store: SharedStore,
}

impl ReadinessCheck for StoreReadiness {
    fn check(&self) -> Pin<Box<dyn Future<Output = Result<(), ManagerError>> + Send + '_>> {
        Box::pin(async move {
            // A successful ping proves the backing store can respond inside the
            // readiness deadline. The endpoint intentionally avoids exposing
            // connection details in the public response body.
            self.store.ping().await?;
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct AppState {
    // A BASE (unscoped) SharedStore. The manager is a multi-tenant control
    // surface, so it must be able to enumerate and address every tenant's
    // namespaced keys (`ns:{tenant}:*`). We therefore keep one unscoped store
    // and build fully-qualified keys in the route layer, rather than pinning a
    // single tenant at construction time like the controller does.
    store: SharedStore,
    // The bearer token required by every authenticated `/api/v1` route. Stored
    // on state so handlers and middleware never read `OPENFLOWS_MANAGER_TOKEN`
    // per request. It is required at startup (see `from_env`) so the manager
    // can never be left open.
    auth_token: String,
    // A trait object keeps handler code stable while allowing tests and future
    // deployments to provide richer readiness behavior.
    readiness: Arc<dyn ReadinessCheck>,
}

impl AppState {
    pub async fn from_env() -> Result<Self, ManagerError> {
        // Reuse the workspace config crate so the manager follows the same
        // environment precedence and tenant defaults as the other OpenFlows
        // components.
        let env = config::EnvConfig::from_env()
            .map_err(|error| ManagerError::Config(error.to_string()))?;
        let redis_url = env.infra.effective_redis_url();

        // The manager is a control surface for many tenants, so it holds an
        // unscoped store and addresses `ns:{tenant}:*` keys explicitly. It must
        // not be constructible without a token: refuse to boot rather than
        // expose an unauthenticated control plane.
        let auth_token = crate::auth::token_from_env()?;
        let store = SharedStore::new_redis(&redis_url).await?;

        Ok(Self::new(store, auth_token))
    }

    pub fn for_tests() -> Self {
        // Tests should exercise router and handler behavior without requiring a
        // Redis instance. The store is unscoped (in-memory) so the multi-tenant
        // routes can be exercised against `ns:*` keys.
        Self::new(SharedStore::new_in_memory(), TEST_AUTH_TOKEN.to_string())
    }

    pub fn new(store: SharedStore, auth_token: String) -> Self {
        // The default readiness implementation mirrors production behavior:
        // report ready only when the shared store can be pinged.
        let readiness = Arc::new(StoreReadiness {
            store: store.clone(),
        });
        Self {
            store,
            auth_token,
            readiness,
        }
    }

    pub fn with_readiness_check(
        store: SharedStore,
        readiness: Arc<dyn ReadinessCheck>,
        auth_token: String,
    ) -> Self {
        // This constructor is intentionally public for smoke tests and any
        // embedding scenarios that need to compose the manager with a custom
        // health policy.
        Self {
            store,
            auth_token,
            readiness,
        }
    }

    /// The bearer token that authenticated `/api/v1` requests must present.
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    pub fn store(&self) -> &SharedStore {
        &self.store
    }

    pub async fn check_readiness(&self) -> Result<(), ManagerError> {
        // Bound readiness latency so a hung dependency cannot make the endpoint
        // hang indefinitely. This keeps orchestrator probes responsive and lets
        // callers distinguish liveness from dependency availability.
        match timeout(READINESS_CHECK_TIMEOUT, self.readiness.check()).await {
            Ok(result) => result,
            Err(_) => Err(ManagerError::Service(anyhow::anyhow!(
                "readiness check timed out after {:?}",
                READINESS_CHECK_TIMEOUT
            ))),
        }
    }
}

pub fn create_router(state: AppState) -> Router {
    // Operational probes stay unauthenticated and version-independent so
    // orchestrators can reach them without credentials. The authenticated
    // product API is mounted under /api/v1 with auth applied at that edge, so
    // the two never share an auth policy.
    let probes = Router::<AppState>::new()
        .route("/health", get(crate::routes::health::health))
        .route("/ready", get(crate::routes::health::ready));

    let api = crate::routes::api_v1_router().layer(axum::middleware::from_fn_with_state(
        state.clone(),
        crate::auth::require_auth,
    ));

    probes.nest("/api/v1", api).with_state(state)
}

pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ManagerError> {
    // Axum owns the accept loop here. The caller supplies the listener so tests
    // can bind to port 0 and production can keep address parsing in main.
    axum::serve(listener, create_router(state))
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}

pub async fn bind_and_serve(
    addr: SocketAddr,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<(), ManagerError> {
    // Binding is split from serving to give tests direct control over listener
    // setup while keeping the binary entry point compact.
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(%addr, "OpenFlows Manager listening");
    serve(listener, state, shutdown).await
}
