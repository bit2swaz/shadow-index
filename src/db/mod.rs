use clickhouse::Client;
use eyre::{Result, WrapErr};

mod writer;
pub use writer::ClickHouseWriter;

pub fn create_client(url: &str) -> Client {
    Client::default()
        .with_url(url)
        .with_database("default")
}

pub struct SchemaManager {
    client: Client,
}

impl SchemaManager {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn initialize_schema(&self) -> Result<()> {
        let schema_sql = include_str!("schema.sql");
        
        for (idx, statement) in schema_sql.split(';').enumerate() {
            let statement = statement.trim();
            
            if statement.is_empty() || statement.starts_with("--") {
                continue;
            }
            
            self.client
                .query(statement)
                .execute()
                .await
                .wrap_err_with(|| format!("failed to execute schema statement {}: {}", idx, statement))?;
        }
        
        tracing::info!("database schema initialized successfully");
        Ok(())
    }

    pub async fn verify_schema(&self) -> Result<()> {
        let required_tables = vec!["blocks", "transactions", "logs", "storage_diffs"];
        
        for table in required_tables {
            let result: Result<u8, clickhouse::error::Error> = self.client
                .query(&format!("EXISTS TABLE {}", table))
                .fetch_one()
                .await;
            
            match result {
                Ok(1) => tracing::debug!("table '{}' exists", table),
                Ok(0) => return Err(eyre::eyre!("required table '{}' does not exist", table)),
                Ok(val) => return Err(eyre::eyre!("unexpected value {} when checking table '{}'", val, table)),
                Err(e) => return Err(eyre::eyre!("failed to check table '{}': {}", table, e)),
            }
        }
        
        tracing::info!("all required tables verified");
        Ok(())
    }

    #[cfg(test)]
    pub async fn drop_all_tables(&self) -> Result<()> {
        let tables = vec!["blocks", "transactions", "logs", "storage_diffs"];
        
        for table in tables {
            self.client
                .query(&format!("DROP TABLE IF EXISTS {}", table))
                .execute()
                .await
                .wrap_err_with(|| format!("failed to drop table {}", table))?;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use testcontainers::{clients::Cli, GenericImage, RunnableImage};
    
    #[test]
    fn test_create_client() {
        let client = create_client("http://localhost:8123");
        drop(client);
    }
    
    fn spawn_clickhouse(docker: &Cli) -> (testcontainers::Container<'_, GenericImage>, u16, String) {
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
    async fn test_clickhouse_connection() {
        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);
        
        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        let client = create_client(&url);
        
        let mut attempts = 0;
        let max_attempts = 10;
        let result = loop {
            attempts += 1;
            match client.query("SELECT 1").fetch_one::<u8>().await {
                Ok(val) => break Ok(val),
                Err(e) if attempts < max_attempts => {
                    println!("attempt {}/{} failed: {}. retrying...", attempts, max_attempts, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    continue;
                }
                Err(e) => break Err(e),
            }
        };
        
        assert!(result.is_ok(), "should successfully execute SELECT 1: {:?}", result.err());
        assert_eq!(result.unwrap(), 1, "query should return 1");
        println!("ClickHouse connection test passed");
    }
    
    #[tokio::test]
    #[ignore]
    async fn test_clickhouse_version() {
        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);
        
        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        let client = create_client(&url);
        
        let mut attempts = 0;
        let max_attempts = 10;
        let result = loop {
            attempts += 1;
            match client.query("SELECT version()").fetch_one::<String>().await {
                Ok(val) => break Ok(val),
                Err(e) if attempts < max_attempts => {
                    println!("attempt {}/{} failed: {}. retrying...", attempts, max_attempts, e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    continue;
                }
                Err(e) => break Err(e),
            }
        };
        
        assert!(result.is_ok(), "should successfully get version: {:?}", result.err());
        let version = result.unwrap();
        assert!(!version.is_empty(), "version should not be empty");
        println!("ClickHouse version: {}", version);
    }

    #[tokio::test]
    #[ignore]
    async fn test_schema_initialization() {
        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);
        
        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client);
        
        let result = schema_manager.initialize_schema().await;
        assert!(result.is_ok(), "schema initialization should succeed: {:?}", result.err());
        
        let verify_result = schema_manager.verify_schema().await;
        assert!(verify_result.is_ok(), "schema verification should succeed: {:?}", verify_result.err());
        
        println!("schema initialization test passed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_schema_drop_and_recreate() {
        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);
        
        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client);
        
        schema_manager.initialize_schema().await.unwrap();
        
        let drop_result = schema_manager.drop_all_tables().await;
        assert!(drop_result.is_ok(), "drop tables should succeed: {:?}", drop_result.err());
        
        let reinit_result = schema_manager.initialize_schema().await;
        assert!(reinit_result.is_ok(), "re-initialization should succeed: {:?}", reinit_result.err());
        
        let verify_result = schema_manager.verify_schema().await;
        assert!(verify_result.is_ok(), "schema verification should succeed: {:?}", verify_result.err());
        
        println!("schema drop and recreate test passed");
    }

    #[tokio::test]
    #[ignore]
    async fn test_table_structure() {
        let docker = Cli::default();
        let (_container, _port, url) = spawn_clickhouse(&docker);
        
        println!("waiting for ClickHouse to initialize...");
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
        
        let client = create_client(&url);
        let schema_manager = SchemaManager::new(client.clone());
        
        schema_manager.initialize_schema().await.unwrap();
        
        let engine_result: Result<String, clickhouse::error::Error> = client
            .query("SELECT engine FROM system.tables WHERE database = 'default' AND name = 'blocks'")
            .fetch_one()
            .await;
        
        assert!(engine_result.is_ok(), "should be able to query engine type");
        let engine = engine_result.unwrap();
        assert!(engine.contains("CollapsingMergeTree"), "engine should be CollapsingMergeTree, got: {}", engine);
        
        println!("table structure test passed");
    }
}
