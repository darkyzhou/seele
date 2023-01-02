use crate::{composer::ComposerQueueItem, entity::SubmissionConfig};
use async_stream::stream;
use futures_util::FutureExt;
use std::{convert::Infallible, net::IpAddr, num::NonZeroUsize, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::{debug, error, info};
use warp::Filter;

pub async fn run_http_exchange(
    handle: SubsystemHandle,
    composer_queue_tx: mpsc::Sender<ComposerQueueItem>,
    addr: IpAddr,
    port: u16,
) -> anyhow::Result<()> {
    const POST_BODY_SIZE_LIMIT: u64 = 1024 * 1024 * 4;

    let routes = warp::post()
        .and(warp::path("submission"))
        .and(warp::body::content_length_limit(POST_BODY_SIZE_LIMIT))
        .and(warp::body::json())
        .and_then(move |submission: SubmissionConfig| {
            use http::{Response, StatusCode};
            use hyper::Body;

            let composer_queue_tx = composer_queue_tx.clone();
            async move {
                let submission = Arc::new(submission);
                let (tx, mut rx) = ring_channel::ring_channel(NonZeroUsize::new(1).unwrap());
                match composer_queue_tx.send((submission.clone(), tx)).await {
                    Err(err) => {
                        error!("Error sending submission to composer_queue_tx: {:#?}", err);
                        Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from(""))
                            .unwrap()
                    }
                    Ok(_) => {
                        let stream = stream! {
                            while let Ok(_) = rx.recv() {
                                match serde_json::to_string(&submission) {
                                    Err(err) => {
                                        error!("Error serializing the submission: {:#?}", err);
                                    }
                                    Ok(json) => {
                                        debug!("Emitting the submission json");
                                        yield Result::<_, Infallible>::Ok(json)
                                    }
                                }
                            }
                        };

                        Response::new(Body::wrap_stream(stream))
                    }
                }
            }
            .map(Result::<_, Infallible>::Ok)
        });

    let (tx, rx) = oneshot::channel();
    let (_, server) = warp::serve(routes).bind_with_graceful_shutdown((addr, port), async move {
        rx.await.ok();
    });

    info!("Running http exchange on {}:{}", addr, port);
    tokio::spawn(server);

    handle.on_shutdown_requested().await;
    let _ = tx.send(());
    Ok(())
}
