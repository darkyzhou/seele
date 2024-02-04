use std::{convert::Infallible, net::SocketAddr, num::NonZeroUsize, time::Duration};

use anyhow::{bail, Result};
use futures_util::StreamExt;
use hyper::{
    body::{self, HttpBody},
    service::{make_service_fn, service_fn},
    *,
};
use ring_channel::ring_channel;
use tokio::time::sleep;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{error, info};

use crate::{
    composer::{ComposerQueueItem, ComposerQueueTx, SubmissionSignal, SubmissionSignalExt},
    conf::HttpExchangeConfig,
};

pub async fn run(
    name: &str,
    handle: SubsystemHandle,
    tx: ComposerQueueTx,
    config: &HttpExchangeConfig,
) -> Result<()> {
    info!("Starting http exchange {} on {}:{}", name, config.address, config.port);

    let service = make_service_fn(move |_| {
        let tx = tx.clone();
        let body_size_limit_bytes = config.max_body_size_bytes;
        async move {
            Ok::<_, Infallible>(service_fn(move |request| {
                let tx = tx.clone();
                async move {
                    Ok::<_, Infallible>(
                        match handle_submission_request(request, tx, body_size_limit_bytes).await {
                            Ok(response) => response,
                            Err(err) => Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from(format!("Internal error: {err:#}")))
                                .unwrap(),
                        },
                    )
                }
            }))
        }
    });

    Server::bind(&SocketAddr::from((config.address, config.port)))
        .serve(service)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;

            info!("Http exchange is shutting down, waiting for unfinished submissions");
            sleep(Duration::from_secs(5)).await;
        })
        .await?;

    Ok(())
}

async fn handle_submission_request(
    request: Request<Body>,
    tx: ComposerQueueTx,
    body_size_limit_bytes: u64,
) -> Result<Response<Body>> {
    {
        let body_size = request.body().size_hint().upper().unwrap_or(body_size_limit_bytes + 1);
        if body_size > body_size_limit_bytes {
            bail!("The size of the request body exceeds the limit: {}", body_size);
        }
    }

    let show_progress = matches!(request.uri().query(), Some(query) if query.contains("progress"));
    let debug = matches!(request.uri().query(), Some(query) if query.contains("debug"));
    let config_yaml = {
        let body = body::to_bytes(request.into_body()).await?;
        String::from_utf8(body.into())?
    };
    let (status_tx, status_rx) = ring_channel(NonZeroUsize::try_from(1).unwrap());
    tx.send(ComposerQueueItem { config_yaml, status_tx }).await?;

    Ok(Response::new(Body::wrap_stream(status_rx.map(move |signal| {
        type CallbackResult = Result<String, Infallible>;

        if !show_progress && matches!(signal.ext, SubmissionSignalExt::Progress { .. }) {
            return CallbackResult::Ok("".to_string());
        }

        fn serialize(debug: bool, signal: &SubmissionSignal) -> String {
            let result = if debug {
                serde_json::to_string_pretty(signal)
            } else {
                serde_json::to_string(signal)
            };
            match result {
                Err(err) => {
                    error!("Error serializing the value: {:#}", err);
                    "".to_string()
                }
                Ok(json) => format!("{}\n", json),
            }
        }

        CallbackResult::Ok(serialize(debug, &signal))
    }))))
}
