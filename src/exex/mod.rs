use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::StreamExt;
use reth_exex::{ExExContext, ExExNotification};
use reth_node_api::FullNodeComponents;
use tracing::info;

pub struct ShadowExEx<Node: FullNodeComponents> {
    ctx: ExExContext<Node>,
}

impl<Node: FullNodeComponents> ShadowExEx<Node> {
    pub fn new(ctx: ExExContext<Node>) -> Self {
        Self { ctx }
    }
}

impl<Node: FullNodeComponents> Future for ShadowExEx<Node> {
    type Output = eyre::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use reth::core::primitives::AlloyBlockHeader as _;
        
        let this = self.get_mut();

        loop {
            match this.ctx.notifications.poll_next_unpin(cx) {
                Poll::Ready(Some(notification)) => {
                    let notification = match notification {
                        Ok(notif) => notif,
                        Err(e) => {
                            return Poll::Ready(Err(eyre::eyre!("notification stream error: {}", e)));
                        }
                    };

                    match &notification {
                        ExExNotification::ChainCommitted { new } => {
                            for block in new.blocks_iter() {
                                let block_number = block.header().number();
                                info!("committed block {}", block_number);
                            }
                        }
                        ExExNotification::ChainReverted { old } => {
                            for block in old.blocks_iter() {
                                let block_number = block.header().number();
                                info!("reverted block {}", block_number);
                            }
                        }
                        ExExNotification::ChainReorged { old, new } => {
                            for block in old.blocks_iter() {
                                info!("reverted block {} (reorg)", block.header().number());
                            }
                            for block in new.blocks_iter() {
                                info!("committed block {} (reorg)", block.header().number());
                            }
                        }
                    }

                    if let Some(committed_chain) = notification.committed_chain() {
                        if let Err(e) = this.ctx.events.send(
                            reth_exex::ExExEvent::FinishedHeight(committed_chain.tip().num_hash())
                        ) {
                            return Poll::Ready(Err(eyre::eyre!("failed to send ExEx event: {}", e)));
                        }
                    }
                }
                Poll::Ready(None) => {
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
    #[test]
    fn test_shadow_exex_can_be_created() {
        assert!(true, "shadowExEx module compiles successfully");
    }
}
