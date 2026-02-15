use metrics_exporter_prometheus::PrometheusBuilder;
use reth::cli::Cli;
use reth_node_ethereum::EthereumNode;
use reth_tracing::Tracer;

mod config;
mod db;
mod exex;
mod transform;
mod utils;

fn main() -> eyre::Result<()> {
    let _guard = reth_tracing::RethTracer::new().init()?;

    let app_config = config::AppConfig::load()
        .expect("failed to load configuration - check config.toml and environment variables");

    tracing::info!("configuration loaded successfully");
    tracing::info!("  clickhouse url: {}", app_config.clickhouse.url);
    tracing::info!("  clickhouse database: {}", app_config.clickhouse.database);
    tracing::info!("  buffer size: {}", app_config.exex.buffer_size);
    tracing::info!("  flush interval: {}ms", app_config.exex.flush_interval_ms);
    tracing::info!("  backfill enabled: {}", app_config.backfill.enabled);
    tracing::info!("  cursor file: {}", app_config.cursor.file_path);

    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], 9001))
        .install()
        .expect("failed to install Prometheus recorder");
    tracing::info!("prometheus metrics endpoint started on 0.0.0.0:9001");

    let cli = Cli::parse_args();

    cli.run(|builder, _| async move {
        let client = db::create_client_from_config(&app_config.clickhouse);

        db::migrations::run_migrations(&client)
            .await
            .expect("failed to run database migrations");

        let cursor = utils::CursorManager::new(&app_config.cursor.file_path)
            .expect("Failed to initialize cursor manager");
        tracing::info!(
            "cursor initialized, last processed block: {}",
            cursor.last_processed_block
        );

        let writer = db::writer::ClickHouseWriter::new(client);

        let exex_config = app_config.exex.clone();

        let handle = builder
            .node(EthereumNode::default())
            .install_exex("shadow-index", move |ctx| async move {
                Ok(exex::ShadowExEx::with_config(
                    ctx,
                    writer,
                    cursor,
                    exex_config.buffer_size,
                    exex_config.flush_interval_ms,
                ))
            })
            .launch()
            .await?;

        handle.wait_for_node_exit().await
    })
}
