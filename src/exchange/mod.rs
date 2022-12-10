use crate::composer::ComposerQueueItem;
use crate::entity::SubmissionConfig;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::error;

pub async fn exchange_main(
    handle: SubsystemHandle,
    mut composer_queue_tx: mpsc::Sender<ComposerQueueItem>,
) -> anyhow::Result<()> {
    Ok(())
}
