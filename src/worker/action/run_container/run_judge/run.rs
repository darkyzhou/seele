use super::{config::ActionRunConfig, MOUNT_DIRECTORY};
use crate::{
    entity::ActionExecutionReport,
    worker::{run_container, runj, ActionContext, MountConfig},
};
use tracing::instrument;

#[instrument]
pub async fn run(
    ctx: &ActionContext,
    config: &ActionRunConfig,
) -> anyhow::Result<ActionExecutionReport> {
    let run_container_config = {
        let mut run_container_config = config.run_container_config.clone();

        run_container_config.mounts.extend(
            config
                .executable
                .iter()
                .map(|file| runj::MountConfig {
                    from: ctx.submission_root.join(file),
                    to: [MOUNT_DIRECTORY, file].iter().collect(),
                    options: Some(vec!["exec".to_string()]),
                })
                .map(MountConfig::Full),
        );

        run_container_config
    };

    run_container(ctx, &run_container_config).await
}
