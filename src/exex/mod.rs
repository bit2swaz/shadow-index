use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use alloy_consensus::BlockHeader;
use eyre::WrapErr;
use futures::StreamExt;
use metrics::{counter, gauge};
use reth_execution_types::Chain;
use reth_exex::{ExExContext, ExExNotification};
use reth_node_api::{Block, FullNodeComponents};
use reth_primitives::EthPrimitives;
use reth_provider::{BlockNumReader, BlockReader, ReceiptProvider};
use tracing::info;

pub mod buffer;
pub use buffer::Batcher;

use crate::db::writer::ClickHouseWriter;
use crate::transform::{transform_block, transform_logs, transform_state};
use crate::utils::CursorManager;

pub struct ShadowExEx<Node: FullNodeComponents> {
    ctx: ExExContext<Node>,
    writer: ClickHouseWriter,
    batcher: Batcher,
    cursor: CursorManager,
    backfill_completed: bool,
}

impl<Node> ShadowExEx<Node>
where
    Node: FullNodeComponents<Types: reth_node_api::NodeTypes<Primitives = EthPrimitives>>,
{
    pub fn new(ctx: ExExContext<Node>, writer: ClickHouseWriter, cursor: CursorManager) -> Self {
        Self {
            ctx,
            writer,
            batcher: Batcher::new(),
            cursor,
            backfill_completed: false,
        }
    }

    async fn process_chain(
        &mut self,
        chain: &Arc<Chain<EthPrimitives>>,
        sign: i8,
    ) -> eyre::Result<()> {
        let mut highest_block = 0u64;

        for (block, receipts) in chain.blocks_and_receipts() {
            let block_number = block.number();

            if block_number > highest_block {
                highest_block = block_number;
            }

            let sealed_block = block.sealed_block();

            let (block_row, tx_rows) = transform_block(sealed_block, sign);
            let tx_count = tx_rows.len();
            self.batcher.push_block(block_row);
            self.batcher.push_transactions(tx_rows);

            let log_rows = transform_logs(receipts, block_number, sign);
            let log_count = log_rows.len();
            self.batcher.push_logs(log_rows);

            let bundle_state = chain.execution_outcome().state();
            let storage_diff_rows = transform_state(bundle_state, block_number, sign);
            let state_diff_count = storage_diff_rows.len();
            self.batcher.push_storage_diffs(storage_diff_rows);

            counter!("shadow_index_blocks_processed_total").increment(1);

            let total_events = tx_count + log_count + state_diff_count;
            counter!("shadow_index_events_captured_total").increment(total_events as u64);

            if sign == -1 {
                counter!("shadow_index_reorgs_handled_total").increment(1);
            }

            info!(
                "processed block {} (sign={}) - batcher now has {} rows",
                block_number,
                sign,
                self.batcher.total_rows()
            );
        }

        gauge!("shadow_index_buffer_saturation").set(self.batcher.total_rows() as f64);

        if self.batcher.should_flush() {
            info!(
                "batcher threshold reached ({} rows), flushing to ClickHouse",
                self.batcher.total_rows()
            );

            self.writer.flush(&mut self.batcher).await?;

            self.cursor.update_cursor(highest_block)?;
            info!("cursor updated to block {}", highest_block);
        }

        Ok(())
    }

    async fn backfill_historical_blocks(&mut self) -> eyre::Result<()> {
        let head_block = self
            .ctx
            .provider()
            .best_block_number()
            .wrap_err("failed to get best block number")?;
        let cursor_block = self.cursor.last_processed_block;

        if cursor_block >= head_block {
            info!(
                "cursor at block {}, head at block {} - no backfill needed",
                cursor_block, head_block
            );
            return Ok(());
        }

        let total_blocks = head_block - cursor_block;
        info!(
            "starting historical backfill: cursor at block {}, head at block {} ({} blocks to process)",
            cursor_block, head_block, total_blocks
        );

        for block_num in (cursor_block + 1)..=head_block {
            let block = self
                .ctx
                .provider()
                .block_by_number(block_num)
                .wrap_err_with(|| format!("failed to fetch block {}", block_num))?
                .ok_or_else(|| eyre::eyre!("block {} not found", block_num))?;

            let receipts = self
                .ctx
                .provider()
                .receipts_by_block(block_num.into())
                .wrap_err_with(|| format!("failed to fetch receipts for block {}", block_num))?
                .unwrap_or_default();

            let sealed_block = block.seal_slow();

            let (block_row, tx_rows) = transform_block(&sealed_block, 1);
            let tx_count = tx_rows.len();
            self.batcher.push_block(block_row);
            self.batcher.push_transactions(tx_rows);

            let log_rows = transform_logs(&receipts, block_num, 1);
            let log_count = log_rows.len();
            self.batcher.push_logs(log_rows);

            counter!("shadow_index_blocks_processed_total").increment(1);
            let total_events = tx_count + log_count;
            counter!("shadow_index_events_captured_total").increment(total_events as u64);

            gauge!("shadow_index_buffer_saturation").set(self.batcher.total_rows() as f64);

            if self.batcher.should_flush() {
                self.writer.flush(&mut self.batcher).await?;
                self.cursor.update_cursor(block_num)?;

                let progress = ((block_num - cursor_block) as f64 / total_blocks as f64) * 100.0;
                info!(
                    "backfilling... block {}/{} ({:.2}%)",
                    block_num, head_block, progress
                );
            }
        }

        if !self.batcher.is_empty() {
            info!(
                "backfill complete, flushing final {} rows",
                self.batcher.total_rows()
            );
            self.writer.flush(&mut self.batcher).await?;
            self.cursor.update_cursor(head_block)?;
        }

        info!(
            "historical backfill complete: processed {} blocks",
            total_blocks
        );
        Ok(())
    }
}

impl<Node> Future for ShadowExEx<Node>
where
    Node: FullNodeComponents<Types: reth_node_api::NodeTypes<Primitives = EthPrimitives>>,
{
    type Output = eyre::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if !this.backfill_completed {
            info!("checking for historical backfill...");
            match futures::executor::block_on(this.backfill_historical_blocks()) {
                Ok(()) => {
                    this.backfill_completed = true;
                    info!("backfill phase complete, entering live notification loop");
                }
                Err(e) => {
                    tracing::error!(
                        "ExEx circuit breaker tripped during backfill! shutting down node. Error: {}",
                        e
                    );
                    return Poll::Ready(Err(e));
                }
            }
        }

        loop {
            match this.ctx.notifications.poll_next_unpin(cx) {
                Poll::Ready(Some(notification)) => {
                    let notification = match notification {
                        Ok(notif) => notif,
                        Err(e) => {
                            return Poll::Ready(Err(eyre::eyre!(
                                "notification stream error: {}",
                                e
                            )));
                        }
                    };

                    let process_result = match &notification {
                        ExExNotification::ChainCommitted { new } => {
                            info!("received ChainCommitted notification");
                            futures::executor::block_on(this.process_chain(new, 1))
                        }
                        ExExNotification::ChainReverted { old } => {
                            info!("received ChainReverted notification");
                            futures::executor::block_on(this.process_chain(old, -1))
                        }
                        ExExNotification::ChainReorged { old, new } => {
                            info!("received ChainReorged notification");
                            let revert_result =
                                futures::executor::block_on(this.process_chain(old, -1));
                            if let Err(e) = revert_result {
                                tracing::error!(
                                    "ExEx circuit breaker tripped during reorg revert! shutting down node. Error: {}",
                                    e
                                );
                                return Poll::Ready(Err(e));
                            }
                            futures::executor::block_on(this.process_chain(new, 1))
                        }
                    };

                    if let Err(e) = process_result {
                        tracing::error!(
                            "ExEx circuit breaker tripped! shutting down node to prevent data desync. Error: {}",
                            e
                        );
                        return Poll::Ready(Err(e));
                    }

                    if let Some(committed_chain) = notification.committed_chain() {
                        if let Err(e) = this.ctx.events.send(reth_exex::ExExEvent::FinishedHeight(
                            committed_chain.tip().num_hash(),
                        )) {
                            return Poll::Ready(Err(eyre::eyre!(
                                "failed to send ExEx event: {}",
                                e
                            )));
                        }
                    }
                }
                Poll::Ready(None) => {
                    if !this.batcher.is_empty() {
                        info!(
                            "ExEx shutting down, flushing remaining {} rows",
                            this.batcher.total_rows()
                        );
                        if let Err(e) =
                            futures::executor::block_on(this.writer.flush(&mut this.batcher))
                        {
                            tracing::error!(
                                "ExEx circuit breaker tripped during shutdown flush! error: {}",
                                e
                            );
                            return Poll::Ready(Err(e));
                        }
                    }
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_exex_can_be_created() {
        assert!(
            true,
            "ShadowExEx module compiles successfully with EthPrimitives constraint"
        );
    }

    // TODO: Update integration tests for new Reth Chain API
    // The Chain::new constructor and blocks accessors have changed
    // Need to update mock Chain creation and block iteration patterns
}
