use std::sync::{Arc, Barrier};

use anyhow::{Context, Result};
use seele_cgroup as cgroup;
use tokio::task::spawn_blocking;

use crate::conf;

pub async fn setup_cgroup() -> Result<()> {
    spawn_blocking(|| -> Result<()> {
        cgroup::check_cgroup_setup().context("Error checking cgroup setup")?;
        cgroup::initialize_cgroup_subtrees().context("Error initializing cgroup subtrees")
    })
    .await??;

    let count = conf::CONFIG.thread_counts.worker + conf::CONFIG.thread_counts.runner;
    let begin_barrier = Arc::new(Barrier::new(count));
    let end_barrier = Arc::new(Barrier::new(count));

    for _ in 0..(count - 1) {
        let begin_barrier = begin_barrier.clone();
        let end_barrier = end_barrier.clone();
        spawn_blocking(move || {
            begin_barrier.wait();
            end_barrier.wait();
        });
    }

    spawn_blocking(move || {
        begin_barrier.wait();
        let result = cgroup::bind_application_threads();
        end_barrier.wait();
        result
    })
    .await?
    .context("Error binding application threads")
}
