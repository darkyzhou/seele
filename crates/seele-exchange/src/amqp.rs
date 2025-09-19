use std::{
    collections::HashMap,
    num::NonZeroUsize,
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use lapin::{Channel, ChannelState, Connection, message::Delivery};
use ring_channel::ring_channel;
use tokio::{
    sync::{Mutex, mpsc},
    time::sleep,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{error, info, warn};
use triggered::Listener;

use crate::{
    composer::{ComposerQueueItem, ComposerQueueTx, SubmissionSignal, SubmissionSignalExt},
    conf::{self, AmqpExchangeConfig, AmqpExchangeReportConfig},
};

static STATUS_MAP: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(Default::default);

pub async fn is_amqp_healthy() -> bool {
    let map = STATUS_MAP.lock().await;
    map.values().all(|ok| *ok)
}

pub async fn run(
    name: &str,
    handle: SubsystemHandle,
    composer_tx: ComposerQueueTx,
    config: &AmqpExchangeConfig,
) -> Result<()> {
    info!("Starting amqp exchange {} for {}", name, config.url.host_str().unwrap_or_default());

    let (trigger, shutdown) = triggered::trigger();
    let cancellation_token = handle.create_cancellation_token();

    tokio::spawn(async move {
        cancellation_token.cancelled().await;
        trigger.trigger();
    });

    loop {
        {
            let mut map = STATUS_MAP.lock().await;
            map.insert(name.to_owned(), false);
        }

        tokio::select! {
            _ = shutdown.clone() => break,
            result = create_channel(config.url.as_str()) => {
                let Ok((channel, connection)) = result else {
                    error!("Failed to create channel, will reconnect soon: {:#}", result.err().unwrap());
                    sleep(Duration::from_secs(3)).await;
                    continue;
                };

                let (tx, mut rx) = mpsc::channel(1);
                let mut events_listener = connection.events_listener();

                tokio::spawn(async move {
                    while let Some(event) = events_listener.next().await {
                        if let lapin::Event::Error(err) = event {
                            let _ = tx.send(err).await;
                            break;
                        }
                    }
                });

                tokio::select! {
                    result = rx.recv() => {
                        if let Some(err) = result {
                            error!("Amqp connection failed, will reconnect soon: {err:#}");
                        }
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                    result = do_consume(name, shutdown.clone(), channel, composer_tx.clone(), config) => {
                        if let Err(err) = result {
                            error!("Error consuming the channel, will restart now: {err:#}");
                            sleep(Duration::from_secs(3)).await;
                            continue;
                        }
                    }
                }

                break;
            }
        }
    }

    shutdown.await;
    Ok(())
}

async fn create_channel(url: &str) -> Result<(Channel, Connection)> {
    let connection = Connection::connect(url, Default::default())
        .await
        .context("Error creating an amqp connection")?;
    Ok((connection.create_channel().await.context("Error creating an amqp channel")?, connection))
}

async fn do_consume(
    name: &str,
    shutdown: Listener,
    channel: Channel,
    tx: ComposerQueueTx,
    config: &AmqpExchangeConfig,
) -> Result<()> {
    channel
        .exchange_declare(
            &config.submission.exchange.name,
            config.submission.exchange.kind.clone(),
            config.submission.exchange.options,
            Default::default(),
        )
        .await
        .context("Error declaring the submission exchange")?;

    channel
        .queue_declare(
            &config.submission.queue,
            config.submission.queue_options,
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

    channel
        .exchange_declare(
            &config.report.exchange.name,
            config.report.exchange.kind.clone(),
            config.report.exchange.options,
            Default::default(),
        )
        .await
        .context("Error declaring the report exchange")?;

    channel
        .basic_qos(conf::CONFIG.thread_counts.runner.try_into()?, Default::default())
        .await
        .context("Error setting channel qos")?;

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

    {
        let mut map = STATUS_MAP.lock().await;
        map.insert(name.to_owned(), true);
    }

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
                {
                    let state = channel.status().state();
                    if !matches!(state, ChannelState::Connected) {
                        warn!("Ignoring the signal due to unexpected channel state: {state:?}");
                        continue;
                    }
                }

                let routing_key = match &signal.ext {
                    SubmissionSignalExt::Progress { .. } => &config.progress_routing_key,
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

                    if !matches!(signal.ext, SubmissionSignalExt::Progress(_)) {
                        delivery.ack(Default::default()).await.context("Error sending the ack")?;
                    }

                    anyhow::Ok(())
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
