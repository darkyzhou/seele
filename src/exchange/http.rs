use crate::{composer::ComposerQueueTx, entity::SubmissionConfig};
use bytes::Buf;
use futures_util::StreamExt;
use http::{Request, Response, StatusCode};
use hyper::{
    body::aggregate,
    service::{make_service_fn, service_fn},
    Body, Server,
};
use std::{
    convert::Infallible,
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
    sync::Arc,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{debug, error, info};

pub async fn run_http_exchange(
    handle: SubsystemHandle,
    tx: ComposerQueueTx,
    addr: IpAddr,
    port: u16,
) -> anyhow::Result<()> {
    const POST_BODY_SIZE_LIMIT: u64 = 1024 * 1024 * 4;

    let service = make_service_fn(move |_| {
        let tx = tx.clone();
        async {
            Ok::<_, Infallible>(service_fn(move |req| {
                let tx = tx.clone();
                async move {
                    Ok::<_, Infallible>(match handle_submission_request(tx, req).await {
                        Ok(response) => response,
                        Err(err) => Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from(format!("Internal error: {:#?}", err)))
                            .unwrap(),
                    })
                }
            }))
        }
    });

    info!("Running http exchange on {}:{}", addr, port);
    Server::bind(&SocketAddr::from((addr, port)))
        .serve(service)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;
        })
        .await?;

    Ok(())
}

async fn handle_submission_request(
    tx: ComposerQueueTx,
    req: Request<Body>,
) -> anyhow::Result<Response<Body>> {
    let body = aggregate(req).await?;
    let submission: Arc<SubmissionConfig> = Arc::new(serde_yaml::from_reader(body.reader())?);

    let (status_tx, status_rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());
    debug!(id = submission.id, "Sending the submission into the composer_tx");
    tx.send((submission.clone(), status_tx)).await?;

    let stream = status_rx.map(move |_| {
        Result::<_, Infallible>::Ok(match serde_yaml::to_string(&submission) {
            Err(err) => {
                error!("Error serializing the submission: {:#?}", err);
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
