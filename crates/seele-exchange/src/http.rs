use std::{convert::Infallible, net::SocketAddr, num::NonZeroUsize, time::Duration};

use anyhow::{Result, bail};
use axum::{
    Router,
    body::{Body, HttpBody, to_bytes},
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::any,
};
use futures_util::StreamExt;
use ring_channel::ring_channel;
use tokio::{net::TcpListener, time::sleep};
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
    let app = Router::new().route(
        "/",
        any({
            let tx = tx.clone();
            let max_body_size_bytes = config.max_body_size_bytes;
            move |request: Request| handle_submission_request(request, tx, max_body_size_bytes)
        }),
    );

    let addr = SocketAddr::from((config.address, config.port));
    let listener = TcpListener::bind(addr).await?;

    info!("Starting http exchange {} on {}:{}", name, config.address, config.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;

            info!("Http exchange is shutting down, waiting for unfinished submissions");
            sleep(Duration::from_secs(5)).await;
        })
        .await?;

    Ok(())
}

fn serialize(debug: bool, signal: &SubmissionSignal) -> String {
    let result =
        if debug { serde_json::to_string_pretty(signal) } else { serde_json::to_string(signal) };
    match result {
        Err(err) => {
            error!("Error serializing the value: {:#}", err);
            "".to_string()
        }
        Ok(json) => format!("{}\n", json),
    }
}

async fn handle_submission_request(
    request: Request,
    tx: ComposerQueueTx,
    max_body_size_bytes: u64,
) -> impl IntoResponse {
    match handle_submission_request_inner(request, tx, max_body_size_bytes).await {
        Ok(response) => (StatusCode::OK, response),
        Err(err) => {
            error!("Error handling the submission request: {:#}", err);
            (StatusCode::INTERNAL_SERVER_ERROR, Response::new(Body::from(err.to_string())))
        }
    }
}

async fn handle_submission_request_inner(
    request: Request,
    tx: ComposerQueueTx,
    max_body_size_bytes: u64,
) -> Result<Response> {
    {
        let body_size = request.body().size_hint().upper().unwrap_or(max_body_size_bytes + 1);
        if body_size > max_body_size_bytes {
            bail!("The size of the request body exceeds the limit: {}", body_size);
        }
    }

    let show_progress = matches!(request.uri().query(), Some(query) if query.contains("progress"));
    let debug = matches!(request.uri().query(), Some(query) if query.contains("debug"));
    let config_yaml =
        { String::from_utf8(to_bytes(request.into_body(), usize::MAX).await?.to_vec())? };
    let (status_tx, status_rx) = ring_channel(NonZeroUsize::try_from(1).unwrap());
    tx.send(ComposerQueueItem { config_yaml, status_tx }).await?;

    let stream = status_rx.map(move |signal| {
        type CallbackResult = Result<String, Infallible>;

        if !show_progress && matches!(signal.ext, SubmissionSignalExt::Progress { .. }) {
            return CallbackResult::Ok("".to_string());
        }

        CallbackResult::Ok(serialize(debug, &signal))
    });

    Ok(Response::new(Body::from_stream(stream)))
}
