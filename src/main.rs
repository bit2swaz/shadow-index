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
        let handle = builder
            .node(EthereumNode::default())
            .install_exex("shadow-index", |ctx| async move {
                Ok(exex::ShadowExEx::new(ctx))
            })
            .launch()
            .await?;

        handle.wait_for_node_exit().await
    })
}

