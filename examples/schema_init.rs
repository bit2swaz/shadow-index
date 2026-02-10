use shadow_index::db::{create_client, SchemaManager};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt::init();

    let clickhouse_url = std::env::var("CLICKHOUSE_URL")
        .unwrap_or_else(|_| "http://localhost:8123".to_string());
    
    println!("connecting to ClickHouse at: {}", clickhouse_url);
    let client = create_client(&clickhouse_url);
    
    let schema_manager = SchemaManager::new(client);
    
    println!("initializing database schema...");
    schema_manager.initialize_schema().await?;
    
    println!("verifying schema...");
    schema_manager.verify_schema().await?;
    
    println!("\nschema initialization complete");
    println!("\ncreated tables:");
    println!("  - blocks (CollapsingMergeTree)");
    println!("  - transactions (CollapsingMergeTree)");
    println!("  - logs (CollapsingMergeTree)");
    println!("  - storage_diffs (CollapsingMergeTree)");
    
    Ok(())
}
