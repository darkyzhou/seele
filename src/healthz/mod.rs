use std::{convert::Infallible, net::SocketAddr};

use anyhow::{Context, Result};
use http::Response;
use hyper::{
    service::{make_service_fn, service_fn},
    Server,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{conf, exchange};

pub async fn healthz_main(handle: SubsystemHandle) -> Result<()> {
    if !conf::CONFIG.healthz.enabled {
        info!("Healthz is disabled");
        return Ok(());
    }

    let service = make_service_fn(move |_| async move {
        Ok::<_, Infallible>(service_fn(move |_request| async move {
            if check_healthz().await {
                Response::builder().status(200).body("ok".to_string())
            } else {
                Response::builder().status(500).body("error".to_string())
            }
        }))
    });

    info!("Running healthz endpoint at port: {}", conf::CONFIG.healthz.port);
    Server::bind(&SocketAddr::from(([0, 0, 0, 0], conf::CONFIG.healthz.port)))
        .serve(service)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;
        })
        .await
        .context("Error in http server")
}

async fn check_healthz() -> bool {
    exchange::is_amqp_healthy().await
}
