use std::net::SocketAddr;

use anyhow::Result;
use axum::{Router, http::StatusCode, response::IntoResponse, routing::any};
use tokio::net::TcpListener;
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::info;

use crate::{conf, exchange};

pub async fn healthz_main(handle: SubsystemHandle) -> Result<()> {
    if !conf::CONFIG.healthz.enabled {
        info!("Healthz is disabled");
        return Ok(());
    }

    let app = Router::new().route("/", any(healthz_handler));

    let addr = SocketAddr::from(([0, 0, 0, 0], conf::CONFIG.healthz.port));
    let listener = TcpListener::bind(addr).await?;

    info!("Running healthz endpoint at port: {}", conf::CONFIG.healthz.port);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            handle.on_shutdown_requested().await;
        })
        .await?;

    Ok(())
}

async fn healthz_handler() -> impl IntoResponse {
    if check_healthz().await {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "error")
    }
}

async fn check_healthz() -> bool {
    exchange::is_amqp_healthy().await
}
