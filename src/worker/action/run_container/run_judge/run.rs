use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::instrument;

use super::MOUNT_DIRECTORY;
use crate::{
    entities::ActionSuccessReportExt,
    worker::{
        run_container::{
            self,
            runj::{self},
        },
        ActionContext,
    },
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub run_container_config: run_container::Config,

    #[serde(default)]
    pub executable: Vec<String>,
}

#[instrument(skip_all, name = "action_run_judge_run_execute")]
pub async fn execute(ctx: &ActionContext, config: &Config) -> Result<ActionSuccessReportExt> {
    let run_container_config = {
        let mut run_container_config = config.run_container_config.clone();

        if !config.executable.is_empty() {
            if let Some(paths) = run_container_config.paths.as_mut() {
                paths.push(MOUNT_DIRECTORY.to_string());
            } else {
                run_container_config.paths = Some(vec![MOUNT_DIRECTORY.to_string()]);
            }

            run_container_config.mounts.extend(
                config
                    .executable
                    .iter()
                    .map(|file| runj::MountConfig {
                        from: ctx.submission_root.join(file),
                        to: [MOUNT_DIRECTORY, file].iter().collect(),
                        options: Some(vec!["exec".to_string()]),
                    })
                    .map(run_container::MountConfig::Full),
            );
        }

        run_container_config
    };

    run_container::execute(ctx, &run_container_config).await
}
