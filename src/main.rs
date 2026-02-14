use reth::cli::Cli;
use reth_node_ethereum::EthereumNode;
use reth_tracing::Tracer;
use metrics_exporter_prometheus::PrometheusBuilder;

mod exex;
mod db;
mod transform;
mod utils;

fn main() -> eyre::Result<()> {
    let _guard = reth_tracing::RethTracer::new().init()?;

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], 9001))
        .install()
        .expect("failed to install Prometheus recorder");
    tracing::info!("prometheus metrics endpoint started on 0.0.0.0:9001");

    let cli = Cli::parse_args();

    cli.run(|builder, _| async move {
        let clickhouse_url = std::env::var("CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://localhost:8123".to_string());

        let client = db::create_client(&clickhouse_url);
        let schema_manager = db::SchemaManager::new(client.clone());
        
        schema_manager.initialize_schema().await
            .expect("Failed to initialize ClickHouse schema");

        let cursor = utils::CursorManager::new("shadow-index.cursor")
            .expect("Failed to initialize cursor manager");
        tracing::info!("cursor initialized, last processed block: {}", cursor.last_processed_block);

        let writer = db::writer::ClickHouseWriter::new(client);

        let handle = builder
            .node(EthereumNode::default())
            .install_exex("shadow-index", move |ctx| async move {
                Ok(exex::ShadowExEx::new(ctx, writer, cursor))
            })
            .launch()
            .await?;

        handle.wait_for_node_exit().await
    })
}

