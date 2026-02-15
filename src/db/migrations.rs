use clickhouse::Client;
use eyre::{Result, WrapErr};

#[derive(Debug, Clone)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub sql: &'static str,
}

pub fn migrations() -> Vec<Migration> {
    vec![Migration {
        version: 1,
        name: "001_initial_schema",
        sql: r#"
CREATE TABLE IF NOT EXISTS blocks (
    block_number UInt64,
    sign Int8,
    hash FixedString(32),
    parent_hash FixedString(32),
    timestamp DateTime64(3),
    gas_used UInt64,
    base_fee UInt128,
    miner FixedString(20),
    state_root FixedString(32),
    extra_data String
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, sign)
PRIMARY KEY block_number;

CREATE TABLE IF NOT EXISTS transactions (
    tx_hash FixedString(32),
    block_number UInt64,
    tx_index UInt32,
    sign Int8,
    from FixedString(20),
    to Nullable(FixedString(20)),
    value UInt256,
    input String,
    status UInt8,
    nonce UInt64,
    gas_limit UInt64,
    gas_price UInt128,
    max_fee_per_gas Nullable(UInt128),
    max_priority_fee_per_gas Nullable(UInt128),
    chain_id UInt64
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_hash, sign)
PRIMARY KEY (block_number, tx_hash);

CREATE TABLE IF NOT EXISTS logs (
    block_number UInt64,
    sign Int8,
    tx_hash FixedString(32),
    tx_index UInt32,
    log_index UInt32,
    address FixedString(20),
    topic0 FixedString(32),
    topics Array(FixedString(32)),
    data String
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, tx_hash, log_index, sign)
PRIMARY KEY (block_number, tx_hash, log_index);

CREATE TABLE IF NOT EXISTS storage_diffs (
    block_number UInt64,
    sign Int8,
    address FixedString(20),
    slot FixedString(32),
    value FixedString(32)
) ENGINE = CollapsingMergeTree(sign)
ORDER BY (block_number, address, slot, sign)
PRIMARY KEY (block_number, address, slot);
"#,
    }]
}

pub async fn run_migrations(client: &Client) -> Result<()> {
    tracing::info!("starting migration runner...");

    let create_migrations_table = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version UInt32,
    name String,
    applied_at DateTime DEFAULT now()
) ENGINE = MergeTree
ORDER BY version
"#;

    client
        .query(create_migrations_table)
        .execute()
        .await
        .wrap_err("failed to create schema_migrations table")?;

    tracing::debug!("schema_migrations tracking table ready");

    let current_version: u32 = match client
        .query("SELECT max(version) FROM schema_migrations")
        .fetch_one::<u32>()
        .await
    {
        Ok(version) => version,
        Err(_) => {
            tracing::debug!("schema_migrations table is empty, starting from version 0");
            0
        }
    };

    tracing::info!("current schema version: {}", current_version);

    let all_migrations = migrations();
    let pending_migrations: Vec<_> = all_migrations
        .into_iter()
        .filter(|m| m.version > current_version)
        .collect();

    if pending_migrations.is_empty() {
        tracing::info!("schema is up to date, no migrations to apply");
        return Ok(());
    }

    tracing::info!(
        "found {} pending migration(s) to apply",
        pending_migrations.len()
    );

    for migration in pending_migrations {
        tracing::info!(
            "applying migration v{}: {}",
            migration.version,
            migration.name
        );

        for (idx, statement) in migration.sql.split(';').enumerate() {
            let statement = statement.trim();

            if statement.is_empty() || statement.starts_with("--") {
                continue;
            }

            client.query(statement).execute().await.wrap_err_with(|| {
                format!(
                    "failed to execute migration v{} statement {}: {}",
                    migration.version, idx, statement
                )
            })?;
        }

        client
            .query("INSERT INTO schema_migrations (version, name) VALUES (?, ?)")
            .bind(migration.version)
            .bind(migration.name)
            .execute()
            .await
            .wrap_err_with(|| {
                format!(
                    "failed to record migration v{} in schema_migrations",
                    migration.version
                )
            })?;

        tracing::info!(
            "✓ migration v{}: {} applied successfully",
            migration.version,
            migration.name
        );
    }

    tracing::info!("all migrations applied successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_clickhouse(
        docker: &testcontainers::clients::Cli,
    ) -> (
        testcontainers::Container<'_, testcontainers::GenericImage>,
        u16,
        String,
    ) {
        use testcontainers::{GenericImage, RunnableImage};

        let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
            .with_exposed_port(8123)
            .with_exposed_port(9000);

        let runnable = RunnableImage::from(image);
        let container = docker.run(runnable);
        let http_port = container.get_host_port_ipv4(8123);
        let http_url = format!("http://localhost:{}", http_port);

        (container, http_port, http_url)
    }

    #[tokio::test]
    #[ignore]
    async fn test_migration_runner() {
        use testcontainers::clients::Cli;

        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);

        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = Client::default().with_url(url).with_database("default");

        run_migrations(&client)
            .await
            .expect("first migration run should succeed");

        let result: Result<String, _> = client
            .query("SELECT name FROM system.tables WHERE database = 'default' AND name = 'blocks'")
            .fetch_one::<String>()
            .await;
        assert!(result.is_ok(), "blocks table should exist");

        let result: Result<String, _> = client
            .query("SELECT name FROM system.tables WHERE database = 'default' AND name = 'transactions'")
            .fetch_one::<String>()
            .await;
        assert!(result.is_ok(), "transactions table should exist");

        let result: Result<String, _> = client
            .query("SELECT name FROM system.tables WHERE database = 'default' AND name = 'logs'")
            .fetch_one::<String>()
            .await;
        assert!(result.is_ok(), "logs table should exist");

        let result: Result<String, _> = client
            .query("SELECT name FROM system.tables WHERE database = 'default' AND name = 'storage_diffs'")
            .fetch_one::<String>()
            .await;
        assert!(result.is_ok(), "storage_diffs table should exist");

        let version: u32 = client
            .query("SELECT max(version) FROM schema_migrations")
            .fetch_one::<u32>()
            .await
            .expect("should fetch current version");
        assert_eq!(version, 1, "current version should be 1");

        run_migrations(&client)
            .await
            .expect("second migration run should succeed");

        let version: u32 = client
            .query("SELECT max(version) FROM schema_migrations")
            .fetch_one::<u32>()
            .await
            .expect("should fetch current version");
        assert_eq!(version, 1, "current version should still be 1");

        let count: u32 = client
            .query("SELECT count(*) FROM schema_migrations")
            .fetch_one::<u32>()
            .await
            .expect("should count migrations");
        assert_eq!(count, 1, "should have exactly one migration record");
    }

    #[tokio::test]
    #[ignore]
    async fn test_migration_resume_from_crash() {
        use testcontainers::clients::Cli;

        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);

        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = Client::default().with_url(url).with_database("default");

        client
            .query(
                r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version UInt32,
    name String,
    applied_at DateTime DEFAULT now()
) ENGINE = MergeTree
ORDER BY version
"#,
            )
            .execute()
            .await
            .expect("should create tracking table");

        run_migrations(&client)
            .await
            .expect("migration should handle empty tracking table");

        let version: u32 = client
            .query("SELECT max(version) FROM schema_migrations")
            .fetch_one::<u32>()
            .await
            .expect("should fetch current version");
        assert_eq!(version, 1, "should have applied migration v1");
    }
}
