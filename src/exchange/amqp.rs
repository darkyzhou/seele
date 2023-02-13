use std::{num::NonZeroUsize, sync::Arc};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use lapin::{message::Delivery, Channel, Connection};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{error, info};

use crate::{
    composer::{ComposerQueueItem, ComposerQueueTx, SubmissionSignal},
    conf::{self, AmqpExchangeConfig, AmqpExchangeReportConfig},
};

pub async fn run(
    handle: SubsystemHandle,
    tx: ComposerQueueTx,
    config: &AmqpExchangeConfig,
) -> Result<()> {
    info!("Starting amqp exchange for {}", config.url.host_str().unwrap_or_default());

    if conf::CONFIG.report_progress && config.report.progress_routing_key.is_empty() {
        bail!("report_progress is enabled but progress_routing_key is not specified");
    }

    let connection = Connection::connect(config.url.as_str(), Default::default())
        .await
        .context("Error creating an amqp connection")?;
    let channel =
        Arc::new(connection.create_channel().await.context("Error creating an amqp channel")?);

    channel
        .exchange_declare(
            &config.submission.exchange.name,
            config.submission.exchange.kind.clone(),
            config.submission.exchange.options.clone(),
            Default::default(),
        )
        .await
        .context("Error declaring the exchange")?;

    channel
        .queue_declare(
            &config.submission.queue,
            config.submission.queue_options.clone(),
            Default::default(),
        )
        .await
        .context("Error declaring the queue")?;

    channel
        .queue_bind(
            &config.submission.queue,
            &config.submission.exchange.name,
            &config.submission.routing_key,
            Default::default(),
            Default::default(),
        )
        .await
        .context("Error binding the queue to the exchange")?;

    let mut consumer = channel
        .basic_consume(
            &config.submission.queue,
            &format!("seele-{}", nano_id::base62::<6>()),
            Default::default(),
            Default::default(),
        )
        .await
        .context("Error consuming the channel")?;

    let report_config = Arc::new(config.report.clone());
    while let Ok(Some(delivery)) = consumer.next().cancel_on_shutdown(&handle).await {
        match delivery {
            Err(err) => {
                error!("Error in the delivery: {err:#}");
            }
            Ok(delivery) => {
                if let Err(err) =
                    handle_delivery(&tx, delivery, channel.clone(), report_config.clone()).await
                {
                    error!("Error handling the delivery: {err:#}");
                }
            }
        }
    }

    Ok(())
}

async fn handle_delivery(
    tx: &ComposerQueueTx,
    delivery: Delivery,
    channel: Arc<Channel>,
    config: Arc<AmqpExchangeReportConfig>,
) -> Result<()> {
    let config_yaml = String::from_utf8(delivery.data.clone())?;
    let (status_tx, mut status_rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());

    tokio::spawn({
        let channel = channel.clone();
        async move {
            while let Some(signal) = status_rx.next().await {
                let routing_key = match &signal {
                    SubmissionSignal::Progress(_) => &config.progress_routing_key,
                    _ => &config.report_routing_key,
                };

                let result = async {
                    let mut data = Vec::with_capacity(128);
                    serde_yaml::to_writer(&mut data, &signal)
                        .context("Error serializing the report")?;

                    channel
                        .basic_publish(
                            &config.exchange.name,
                            routing_key,
                            Default::default(),
                            &data,
                            Default::default(),
                        )
                        .await
                        .context("Error publishing the report")?
                        .await
                        .context("Error awaiting the confirmation")?;

                    delivery.ack(Default::default()).await.context("Error sending the ack")
                }
                .await;

                if let Err(err) = result {
                    error!("Error handling the submission: {err:#}");
                }
            }
        }
    });

    tx.send(ComposerQueueItem { config_yaml, status_tx }).await?;

    Ok(())
}
