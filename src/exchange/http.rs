use crate::{composer::ComposerQueueTx, conf::HttpExchangeConfig, entity::SubmissionConfig};
use anyhow::bail;
use bytes::Buf;
use futures_util::StreamExt;
use http::{Request, Response, StatusCode};
use hyper::{
    body::{aggregate, HttpBody},
    service::{make_service_fn, service_fn},
    Body, Server,
};
use std::{convert::Infallible, net::SocketAddr, num::NonZeroUsize, sync::Arc};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{debug, error, info};

pub async fn run_http_exchange(
    handle: SubsystemHandle,
    tx: ComposerQueueTx,
    config: &HttpExchangeConfig,
) -> anyhow::Result<()> {
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
                                .body(Body::from(format!("Internal error: {:#}", err)))
                                .unwrap(),
                        },
                    )
                }
            }))
        }
    });

    info!("Running http exchange on {}:{}", config.address, config.port);
    Server::bind(&SocketAddr::from((config.address, config.port)))
        .serve(service)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;
        })
        .await?;

    Ok(())
}

async fn handle_submission_request(
    request: Request<Body>,
    tx: ComposerQueueTx,
    body_size_limit_bytes: u64,
) -> anyhow::Result<Response<Body>> {
    let body_size = request.body().size_hint().upper().unwrap_or(body_size_limit_bytes + 1);
    if body_size > body_size_limit_bytes {
        bail!("The size of the request body exceeds the limit: {}", body_size);
    }

    let body = aggregate(request).await?;
    let submission: Arc<SubmissionConfig> = Arc::new(serde_yaml::from_reader(body.reader())?);

    let (status_tx, status_rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());
    debug!(id = submission.id, "Sending the submission into the composer_tx");
    tx.send((submission.clone(), status_tx)).await?;

    let stream = status_rx.map(move |_| {
        Result::<_, Infallible>::Ok(match serde_yaml::to_string(&submission) {
            Err(err) => {
                error!("Error serializing the submission: {:#}", err);
                "".to_string()
            }
            Ok(json) => {
                debug!("Emitting the submission json");
                json
            }
        })
    });
    Ok(Response::new(Body::wrap_stream(stream)))
}
