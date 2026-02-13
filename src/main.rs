use reth::cli::Cli;
use reth_node_ethereum::EthereumNode;
use reth_tracing::Tracer;

mod exex;
mod db;
mod transform;
mod utils;

fn main() -> eyre::Result<()> {
    let _guard = reth_tracing::RethTracer::new().init()?;

    let cli = Cli::parse_args();

    cli.run(|builder, _| async move {
        // Get ClickHouse URL from environment or use default
        let clickhouse_url = std::env::var("CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://localhost:8123".to_string());

        // Create ClickHouse client and initialize schema
        let client = db::create_client(&clickhouse_url);
        let schema_manager = db::SchemaManager::new(client.clone());
        
        // Initialize database schema on startup
        schema_manager.initialize_schema().await
            .expect("Failed to initialize ClickHouse schema");

        // Create writer for the ExEx
        let writer = db::writer::ClickHouseWriter::new(client);

        let handle = builder
            .node(EthereumNode::default())
            .install_exex("shadow-index", move |ctx| async move {
                Ok(exex::ShadowExEx::new(ctx, writer))
            })
            .launch()
            .await?;

        handle.wait_for_node_exit().await
    })
}

