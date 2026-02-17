use clickhouse::{Client, Row};
use eyre::{Result, WrapErr};
use metrics::{counter, histogram};
use serde::Serialize;

pub struct ClickHouseWriter {
    client: Client,
}

impl ClickHouseWriter {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn insert_batch<T>(&self, table: &str, rows: &[T]) -> Result<()>
    where
        T: Row + Serialize,
    {
        if rows.is_empty() {
            tracing::debug!("skipping insert: no rows provided for table '{}'", table);
            return Ok(());
        }

        const MAX_ATTEMPTS: u32 = 5;
        const BASE_DELAY_SECS: u64 = 1;
        const BACKOFF_FACTOR: u64 = 2;

        let mut attempt = 0;
        let mut last_error = None;

        while attempt < MAX_ATTEMPTS {
            attempt += 1;

            match self.try_insert_batch(table, rows).await {
                Ok(_) => {
                    if attempt > 1 {
                        tracing::info!(
                            "successfully inserted {} rows into table '{}' after {} attempts",
                            rows.len(),
                            table,
                            attempt
                        );
                    } else {
                        tracing::debug!(
                            "successfully inserted {} rows into table '{}'",
                            rows.len(),
                            table
                        );
                    }
                    return Ok(());
                }
                Err(e) => {
                    if !Self::is_retryable_error(&e) {
                        tracing::error!("non-retryable error writing to table '{}': {}", table, e);
                        return Err(e.wrap_err("permanent database error - not retrying"));
                    }

                    last_error = Some(e);

                    if attempt < MAX_ATTEMPTS {
                        counter!("shadow_index_db_retries_total").increment(1);

                        let delay_secs = BASE_DELAY_SECS * BACKOFF_FACTOR.pow(attempt - 1);
                        tracing::warn!(
                            "DB write to table '{}' failed (attempt {}/{}). retrying in {}s... Error: {}",
                            table,
                            attempt,
                            MAX_ATTEMPTS,
                            delay_secs,
                            last_error.as_ref().unwrap()
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                    }
                }
            }
        }

        counter!("shadow_index_circuit_breaker_trips_total").increment(1);

        Err(eyre::eyre!(
            "CRITICAL: ClickHouse unreachable after {} attempts. circuit breaker tripped. last error: {}",
            MAX_ATTEMPTS,
            last_error.unwrap()
        ))
    }

    async fn try_insert_batch<T>(&self, table: &str, rows: &[T]) -> Result<()>
    where
        T: Row + Serialize,
    {
        let start = std::time::Instant::now();

        let mut insert = self
            .client
            .insert(table)
            .wrap_err_with(|| format!("failed to create insert statement for table '{}'", table))?;

        for row in rows {
            insert
                .write(row)
                .await
                .wrap_err_with(|| format!("failed to write row to table '{}'", table))?;
        }

        insert
            .end()
            .await
            .wrap_err_with(|| format!("failed to finalize insert into table '{}'", table))?;

        histogram!("shadow_index_db_latency_seconds").record(start.elapsed().as_secs_f64());

        Ok(())
    }

    fn is_retryable_error(error: &eyre::Report) -> bool {
        let error_string = format!("{:?}", error);

        let permanent_error_codes = [
            "Code: 516", // Authentication failed
            "Code: 62",  // Syntax error
            "Code: 160", // Query/data too large
            "Code: 60",  // Table doesn't exist
            "Code: 81",  // Database doesn't exist
            "Code: 36",  // Primary key violation / Schema mismatch
            "Code: 57",  // Table already exists (shouldn't happen but permanent)
            "Code: 47",  // Unknown identifier
        ];

        for code in &permanent_error_codes {
            if error_string.contains(code) {
                return false;
            }
        }

        let transient_patterns = [
            "connection refused",
            "connection reset",
            "timeout",
            "timed out",
            "broken pipe",
            "network",
            "503",
            "502",
        ];

        for pattern in &transient_patterns {
            if error_string.to_lowercase().contains(pattern) {
                return true;
            }
        }

        true
    }

    #[allow(dead_code)]
    pub fn client(&self) -> &Client {
        &self.client
    }

    pub async fn flush(&self, batcher: &mut crate::exex::buffer::Batcher) -> Result<()> {
        let total_rows = batcher.total_rows();

        if total_rows == 0 {
            tracing::debug!("flush called but batcher is empty, skipping");
            return Ok(());
        }

        tracing::info!("flushing {} total rows to ClickHouse", total_rows);

        if !batcher.blocks.is_empty() {
            self.insert_batch("blocks", &batcher.blocks)
                .await
                .wrap_err("failed to insert blocks")?;
        }

        if !batcher.transactions.is_empty() {
            self.insert_batch("transactions", &batcher.transactions)
                .await
                .wrap_err("failed to insert transactions")?;
        }

        if !batcher.logs.is_empty() {
            self.insert_batch("logs", &batcher.logs)
                .await
                .wrap_err("failed to insert logs")?;
        }

        if !batcher.storage_diffs.is_empty() {
            self.insert_batch("storage_diffs", &batcher.storage_diffs)
                .await
                .wrap_err("failed to insert storage_diffs")?;
        }

        batcher.clear();

        tracing::info!("successfully flushed {} rows to ClickHouse", total_rows);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clickhouse::Row;
    use serde::Serialize;

    #[derive(Row, Serialize, Debug, Clone)]
    struct TestBlockRow {
        block_number: u64,
        sign: i8,
        hash: Vec<u8>,
        parent_hash: Vec<u8>,
        timestamp: i64,
        gas_used: u64,
        base_fee: u128,
        miner: Vec<u8>,
        state_root: Vec<u8>,
        extra_data: String,
    }

    impl TestBlockRow {
        fn new(block_number: u64, sign: i8) -> Self {
            Self {
                block_number,
                sign,
                hash: vec![0u8; 32],
                parent_hash: vec![0u8; 32],
                timestamp: 1640000000000,
                gas_used: 1000000,
                base_fee: 10000000000,
                miner: vec![0u8; 20],
                state_root: vec![0u8; 32],
                extra_data: "test".to_string(),
            }
        }
    }

    #[test]
    fn test_writer_creation() {
        use crate::db::create_client;
        let client = create_client("http://localhost:8123");
        let writer = ClickHouseWriter::new(client);
        drop(writer);
    }

    #[tokio::test]
    #[ignore]
    async fn test_insert_batch_integration() {
        use crate::db::{create_client, SchemaManager};
        use testcontainers::{clients::Cli, GenericImage, RunnableImage};

        let docker = Cli::default();
        let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
            .with_exposed_port(8123)
            .with_exposed_port(9000);
        let runnable = RunnableImage::from(image);
        let container = docker.run(runnable);
        let http_port = container.get_host_port_ipv4(8123);
        let url = format!("http://localhost:{}", http_port);

        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client.clone());
        schema_manager.initialize_schema().await.unwrap();

        let writer = ClickHouseWriter::new(client.clone());

        let mut rows = Vec::new();
        for i in 0..10 {
            rows.push(TestBlockRow::new(i, 1));
        }

        let result = writer.insert_batch("blocks", &rows).await;
        assert!(result.is_ok(), "insert should succeed: {:?}", result.err());

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let count: Result<u64, clickhouse::error::Error> = client
            .query("SELECT count(*) FROM blocks WHERE sign = 1")
            .fetch_one()
            .await;

        assert!(
            count.is_ok(),
            "count query should succeed: {:?}",
            count.err()
        );
        assert_eq!(count.unwrap(), 10, "should have inserted 10 rows");

        println!("insert batch integration test passed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_insert_with_reorg() {
        use crate::db::{create_client, SchemaManager};
        use testcontainers::{clients::Cli, GenericImage, RunnableImage};

        let docker = Cli::default();
        let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
            .with_exposed_port(8123)
            .with_exposed_port(9000);
        let runnable = RunnableImage::from(image);
        let container = docker.run(runnable);
        let http_port = container.get_host_port_ipv4(8123);
        let url = format!("http://localhost:{}", http_port);

        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client.clone());
        schema_manager.initialize_schema().await.unwrap();

        let writer = ClickHouseWriter::new(client.clone());

        let mut commit_rows = Vec::new();
        for i in 100..105 {
            commit_rows.push(TestBlockRow::new(i, 1));
        }
        writer.insert_batch("blocks", &commit_rows).await.unwrap();

        let mut revert_rows = Vec::new();
        for i in 100..105 {
            revert_rows.push(TestBlockRow::new(i, -1));
        }
        writer.insert_batch("blocks", &revert_rows).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let total_rows: Result<u64, clickhouse::error::Error> = client
            .query("SELECT count(*) FROM blocks WHERE block_number >= 100 AND block_number < 105")
            .fetch_one()
            .await;

        assert!(total_rows.is_ok(), "count query should succeed");
        let count = total_rows.unwrap();
        assert_eq!(
            count, 10,
            "should have 10 rows before collapsing (5 commits + 5 reverts)"
        );

        let sign_sum: Result<i64, clickhouse::error::Error> = client
            .query("SELECT sum(sign) FROM blocks WHERE block_number >= 100 AND block_number < 105")
            .fetch_one()
            .await;

        assert!(sign_sum.is_ok(), "sign sum query should succeed");
        assert_eq!(
            sign_sum.unwrap(),
            0,
            "sum of signs should be 0 (commits cancelled by reverts)"
        );

        println!("reorg test passed - CollapsingMergeTree logic verified");
    }

    #[tokio::test]
    async fn test_circuit_breaker_retry_logic() {
        use std::time::Instant;

        let invalid_url = "http://localhost:9999";
        let client = crate::db::create_client(invalid_url);
        let writer = ClickHouseWriter::new(client);

        let test_rows = vec![TestBlockRow::new(1, 1)];

        let start = Instant::now();
        let result = writer.insert_batch("blocks", &test_rows).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "should fail after max retries");

        let error_msg = result.unwrap_err().to_string();

        assert!(
            error_msg.contains("CRITICAL: ClickHouse unreachable after 5 attempts"),
            "error should mention circuit breaker and 5 attempts. Got: {}",
            error_msg
        );

        let min_expected_secs = 14;
        let elapsed_secs = elapsed.as_secs();

        assert!(
            elapsed_secs >= min_expected_secs,
            "should take at least {} seconds with exponential backoff. took {} seconds",
            min_expected_secs,
            elapsed_secs
        );

        println!(
            "circuit breaker test passed: failed after 5 retries with exponential backoff in {} seconds",
            elapsed_secs
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_empty_batch() {
        use crate::db::{create_client, SchemaManager};
        use testcontainers::{clients::Cli, GenericImage, RunnableImage};

        let docker = Cli::default();
        let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
            .with_exposed_port(8123)
            .with_exposed_port(9000);
        let runnable = RunnableImage::from(image);
        let container = docker.run(runnable);
        let http_port = container.get_host_port_ipv4(8123);
        let url = format!("http://localhost:{}", http_port);

        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client.clone());
        schema_manager.initialize_schema().await.unwrap();

        let writer = ClickHouseWriter::new(client);

        let empty_rows: Vec<TestBlockRow> = vec![];
        let result = writer.insert_batch("blocks", &empty_rows).await;

        assert!(result.is_ok(), "empty batch insert should succeed");
        println!("empty batch test passed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_large_batch() {
        use crate::db::{create_client, SchemaManager};
        use testcontainers::{clients::Cli, GenericImage, RunnableImage};

        let docker = Cli::default();
        let image = GenericImage::new("clickhouse/clickhouse-server", "23.3")
            .with_exposed_port(8123)
            .with_exposed_port(9000);
        let runnable = RunnableImage::from(image);
        let container = docker.run(runnable);
        let http_port = container.get_host_port_ipv4(8123);
        let url = format!("http://localhost:{}", http_port);

        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client.clone());
        schema_manager.initialize_schema().await.unwrap();

        let writer = ClickHouseWriter::new(client.clone());

        let mut rows = Vec::new();
        for i in 0..1000 {
            rows.push(TestBlockRow::new(i, 1));
        }

        let result = writer.insert_batch("blocks", &rows).await;
        assert!(
            result.is_ok(),
            "large batch insert should succeed: {:?}",
            result.err()
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        let count: Result<u64, clickhouse::error::Error> = client
            .query("SELECT count(*) FROM blocks WHERE sign = 1")
            .fetch_one()
            .await;

        assert!(count.is_ok(), "count query should succeed");
        assert_eq!(count.unwrap(), 1000, "should have inserted 1000 rows");

        println!("large batch test passed");
    }
}
