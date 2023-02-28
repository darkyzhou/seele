use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use lapin::{message::Delivery, options::BasicNackOptions, Channel, Connection};
use ring_channel::ring_channel;
use tokio::time::sleep;
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{error, info};
use triggered::Listener;

use crate::{
    composer::{
        ComposerQueueItem, ComposerQueueTx, SubmissionCompletedSignal, SubmissionSignal,
        SubmissionSignalExt,
    },
    conf::{self, AmqpExchangeConfig, AmqpExchangeReportConfig},
};

pub async fn run(
    name: &str,
    handle: SubsystemHandle,
    tx: ComposerQueueTx,
    config: &AmqpExchangeConfig,
) -> Result<()> {
    if conf::CONFIG.composer.report_progress && config.report.progress_routing_key.is_empty() {
        bail!("report_progress is enabled but progress_routing_key is not specified");
    }

    {
        info!(name = name, "Preparing exchanges and queues");

        let channel = create_channel(config.url.as_str()).cancel_on_shutdown(&handle).await??;

        channel
            .exchange_declare(
                &config.submission.exchange.name,
                config.submission.exchange.kind.clone(),
                config.submission.exchange.options.clone(),
                Default::default(),
            )
            .cancel_on_shutdown(&handle)
            .await?
            .context("Error declaring the submission exchange")?;

        channel
            .queue_declare(
                &config.submission.queue,
                config.submission.queue_options.clone(),
                Default::default(),
            )
            .cancel_on_shutdown(&handle)
            .await?
            .context("Error declaring the queue")?;

        channel
            .queue_bind(
                &config.submission.queue,
                &config.submission.exchange.name,
                &config.submission.routing_key,
                Default::default(),
                Default::default(),
            )
            .cancel_on_shutdown(&handle)
            .await?
            .context("Error binding the queue to the exchange")?;

        channel
            .exchange_declare(
                &config.report.exchange.name,
                config.report.exchange.kind.clone(),
                config.report.exchange.options.clone(),
                Default::default(),
            )
            .cancel_on_shutdown(&handle)
            .await?
            .context("Error declaring the report exchange")?;
    }

    let (trigger, shutdown) = triggered::trigger();
    tokio::spawn(async move {
        handle.on_shutdown_requested().await;
        trigger.trigger();
    });

    loop {
        tokio::select! {
            _ = shutdown.clone() => break,
            channel = create_channel(config.url.as_str()) => {
                if let Err(err) = do_consume(name, shutdown.clone(), channel?, tx.clone(), config).await {
                    error!("Error consuming the channel, will restart soon: {err:#}");
                    sleep(Duration::from_secs(3)).await;
                    continue;
                }

                break;
            }
        }
    }

    shutdown.await;
    Ok(())
}

async fn create_channel(url: &str) -> Result<Channel> {
    let connection = Connection::connect(url, Default::default())
        .await
        .context("Error creating an amqp connection")?;
    connection.create_channel().await.context("Error creating an amqp channel")
}

async fn do_consume(
    name: &str,
    shutdown: Listener,
    channel: Channel,
    tx: ComposerQueueTx,
    config: &AmqpExchangeConfig,
) -> Result<()> {
    info!("Starting amqp exchange {} for {}", name, config.url.host_str().unwrap_or_default());

    let channel = Arc::new(channel);

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
    loop {
        tokio::select! {
            _ = shutdown.clone() => break,
            result = consumer.next() => match result {
                None => break,
                Some(Err(err)) => bail!("Failed to consume from the queue: {err:#}"),
                Some(Ok(delivery)) => {
                    if let Err(err) = handle_delivery(&tx, delivery, channel.clone(), report_config.clone()).await {
                        error!("Error handling the delivery: {err:#}");
                    }
                }
            }
        }
    }

    info!("Shutting down, waiting for unfinished submissions");
    sleep(Duration::from_secs(5)).await;
    _ = channel.close(302, "Seele is shutting down").await;

    Ok(())
}

async fn handle_delivery(
    tx: &ComposerQueueTx,
    delivery: Delivery,
    channel: Arc<Channel>,
    config: Arc<AmqpExchangeReportConfig>,
) -> Result<()> {
    let config_yaml = String::from_utf8(delivery.data.clone())?;
    let (status_tx, mut status_rx) =
        ring_channel::<SubmissionSignal>(NonZeroUsize::try_from(1).unwrap());

    tx.send(ComposerQueueItem { config_yaml, status_tx }).await?;

    tokio::spawn({
        let channel = channel.clone();
        async move {
            while let Some(signal) = status_rx.next().await {
                let routing_key = match &signal.ext {
                    SubmissionSignalExt::Progress(_) => &config.progress_routing_key,
                    _ => &config.report_routing_key,
                };

                let result = async {
                    let mut data = Vec::with_capacity(4096);
                    serde_json::to_writer(&mut data, &signal)
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

                    match &signal.ext {
                        SubmissionSignalExt::Completed(SubmissionCompletedSignal::ParseError {
                            ..
                        }) => delivery
                            .nack(BasicNackOptions { requeue: false, ..Default::default() })
                            .await
                            .context("Error sending the nack"),
                        SubmissionSignalExt::Completed(_) => {
                            delivery.ack(Default::default()).await.context("Error sending the ack")
                        }
                        _ => Ok(()),
                    }
                }
                .await;

                if let Err(err) = result {
                    error!("Error handling the submission: {err:#}");
                }
            }
        }
    });

    Ok(())
}
