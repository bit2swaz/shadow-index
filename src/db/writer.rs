use clickhouse::{Client, Row};
use eyre::{Result, WrapErr};
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

        let mut insert = self.client.insert(table)?;
        
        for row in rows {
            insert.write(row).await
                .wrap_err_with(|| format!("failed to write row to table '{}'", table))?;
        }
        
        insert.end().await
            .wrap_err_with(|| format!("failed to finalize insert into table '{}'", table))?;
        
        tracing::debug!("successfully inserted {} rows into table '{}'", rows.len(), table);
        Ok(())
    }

    pub fn client(&self) -> &Client {
        &self.client
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

        assert!(count.is_ok(), "count query should succeed: {:?}", count.err());
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
        assert_eq!(count, 10, "should have 10 rows before collapsing (5 commits + 5 reverts)");

        let sign_sum: Result<i64, clickhouse::error::Error> = client
            .query("SELECT sum(sign) FROM blocks WHERE block_number >= 100 AND block_number < 105")
            .fetch_one()
            .await;

        assert!(sign_sum.is_ok(), "sign sum query should succeed");
        assert_eq!(sign_sum.unwrap(), 0, "sum of signs should be 0 (commits cancelled by reverts)");

        println!("reorg test passed - CollapsingMergeTree logic verified");
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
        assert!(result.is_ok(), "large batch insert should succeed: {:?}", result.err());

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
