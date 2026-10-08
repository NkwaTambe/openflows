//! Shared helpers for PostgreSQL integration tests.
//!
//! Each test creates an isolated database in the Openflows control-plane test
//! server, runs the numbered migrations into it, and drops it on teardown.
//! Tests are truly integration tests against PostgreSQL — not SQLite or mocks —
//! and exercise the real migration runner, constraints, and query paths.
//!
//! The test server is selected with `OPENFLOWS_TEST_DATABASE_URL`, defaulting to
//! the local `openflows-db` Postgres provisioned for WP-01:
//! `postgres://openflows:openflows@localhost:5544/openflows_control_plane`. The
//! test role must be able to create and drop databases (the bundled role is a
//! superuser).

use openflows_manager::error::ManagerError;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use std::time::Duration;

pub const DEFAULT_TEST_DB_URL: &str =
    "postgres://openflows:openflows@localhost:5544/openflows_control_plane";

/// The environment variable that selects the test database URL.
pub const TEST_DB_URL_ENV: &str = "OPENFLOWS_TEST_DATABASE_URL";

fn test_db_url() -> String {
    std::env::var(TEST_DB_URL_ENV).unwrap_or_else(|_| DEFAULT_TEST_DB_URL.to_string())
}

/// Derive an admin connection URL on the same server. We connect to the
/// `postgres` maintenance database to create/drop databases.
fn admin_url(db_url: &str) -> String {
    if let Some((head, _)) = db_url.rsplit_once('/') {
        format!("{head}/postgres")
    } else {
        db_url.to_string()
    }
}

/// An isolated test database with migrations applied.
pub struct TestDb {
    pool: PgPool,
    db_name: String,
    admin_url: String,
}

impl TestDb {
    /// Create a unique database, apply migrations, and return a pool scoped to
    /// it. The database is dropped on teardown.
    pub async fn new() -> Result<Self, ManagerError> {
        let db_url = test_db_url();
        let admin = admin_url(&db_url);
        let db_name = format!("of_test_{:x}", uuid::Uuid::new_v4().as_u128());

        // Create the database on the server (the role must have CREATEDB).
        let mut conn = PgConnection::connect(&admin)
            .await
            .map_err(|e| ManagerError::Service(anyhow::anyhow!("admin connect failed: {e}")))?;
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
            .execute(&mut conn)
            .await
            .map_err(ManagerError::from)?;
        conn.close().await.map_err(ManagerError::from)?;

        // Build a pool connected to the new database (migrations and queries
        // run in its default `public` schema — no search_path juggling needed).
        let (head, _) = db_url
            .rsplit_once('/')
            .ok_or_else(|| ManagerError::Config("invalid test db url".into()))?;
        let db_url_for_pool = format!("{head}/{db_name}");
        let options: PgConnectOptions = db_url_for_pool
            .parse()
            .map_err(|e| ManagerError::Config(format!("invalid test db url: {e}")))?;
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect_with(options)
            .await
            .map_err(|e| ManagerError::Service(anyhow::anyhow!("test pool connect failed: {e}")))?;

        Ok(TestDb {
            pool,
            db_name,
            admin_url: admin,
        })
    }

    /// Run the numbered migrations into the isolated database.
    pub async fn migrate(&self) -> Result<(), ManagerError> {
        openflows_manager::db::MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|e| ManagerError::Migration(e.to_string()))
    }

    /// The number of applied (successful) migrations.
    pub async fn applied_migrations(&self) -> Result<i64, ManagerError> {
        let row: (i64,) =
            sqlx::query_as("SELECT count(*) FROM _sqlx_migrations WHERE success = true")
                .fetch_one(&self.pool)
                .await
                .map_err(ManagerError::from)?;
        Ok(row.0)
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        // Best-effort database cleanup on a dedicated thread with its own
        // runtime. We cannot use `block_in_place`/`Handle::current()` here
        // because the test runs on a single-threaded tokio runtime, where
        // blocking inside a destructor panics. `DROP DATABASE ... WITH (FORCE)`
        // terminates any lingering pool connections (Postgres 13+).
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let mut conn = match PgConnection::connect(&admin_url).await {
                    Ok(conn) => conn,
                    Err(_) => return,
                };
                let _ = sqlx::query(&format!(
                    "DROP DATABASE IF EXISTS \"{db_name}\" WITH (FORCE)"
                ))
                .execute(&mut conn)
                .await;
                let _ = conn.close().await;
            });
        });
    }
}

/// Insert a user row, returning its id.
pub async fn insert_user(pool: &PgPool) -> Result<uuid::Uuid, ManagerError> {
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO users (id, display_name, status) VALUES ($1, $2, 'active')")
        .bind(id)
        .bind("Test User")
        .execute(pool)
        .await
        .map_err(ManagerError::from)?;
    Ok(id)
}

/// Insert an organization and its owner/admin membership atomically, returning
/// the organization id. Satisfies the deferred owner FK.
pub async fn insert_organization_with_owner(
    pool: &PgPool,
    slug: &str,
    owner: uuid::Uuid,
) -> Result<uuid::Uuid, ManagerError> {
    let org = uuid::Uuid::new_v4();
    let mut tx = pool.begin().await.map_err(ManagerError::from)?;
    sqlx::query(
        "INSERT INTO organizations (id, slug, display_name, owner_user_id, status)
         VALUES ($1, $2, $3, $4, 'ready')",
    )
    .bind(org)
    .bind(slug)
    .bind(slug)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .map_err(ManagerError::from)?;
    sqlx::query(
        "INSERT INTO memberships (organization_id, user_id, role, status)
         VALUES ($1, $2, 'admin', 'active')",
    )
    .bind(org)
    .bind(owner)
    .execute(&mut *tx)
    .await
    .map_err(ManagerError::from)?;
    tx.commit().await.map_err(ManagerError::from)?;
    Ok(org)
}

/// Insert a GitHub connection belonging to `org`, returning its id.
pub async fn insert_connection(
    pool: &PgPool,
    org: uuid::Uuid,
    installation_id: i64,
) -> Result<uuid::Uuid, ManagerError> {
    let id = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO github_connections
            (id, organization_id, app_id, installation_id, github_account_id, account_type, status)
         VALUES ($1, $2, 1001, $3, $4, 'Organization', 'active')",
    )
    .bind(id)
    .bind(org)
    .bind(installation_id)
    .bind(installation_id + 1000)
    .execute(pool)
    .await
    .map_err(ManagerError::from)?;
    Ok(id)
}

/// Insert a repository owned by a connection, returning the repo id.
pub async fn insert_repository(
    pool: &PgPool,
    org: uuid::Uuid,
    connection_id: uuid::Uuid,
    github_repository_id: i64,
    full_name: &str,
) -> Result<(), ManagerError> {
    sqlx::query(
        "INSERT INTO github_repositories
            (organization_id, connection_id, github_repository_id, full_name, accessible)
         VALUES ($1, $2, $3, $4, true)",
    )
    .bind(org)
    .bind(connection_id)
    .bind(github_repository_id)
    .bind(full_name)
    .execute(pool)
    .await
    .map_err(ManagerError::from)?;
    Ok(())
}
