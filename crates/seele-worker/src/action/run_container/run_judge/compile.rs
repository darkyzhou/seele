use std::{
    collections::HashMap, fs::Permissions, os::unix::prelude::PermissionsExt, path::Path, sync::Arc,
};

use anyhow::{Context, Result, bail};
use seele_shared::entities::{
    ActionReportExt, ActionSuccessReportExt,
    run_container::{
        self,
        run_judge::compile::{Config, ExecutionReport},
        runj,
    },
};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    fs::{File, OpenOptions},
    io::{self, AsyncReadExt, BufWriter},
    task::spawn_blocking,
};
use tracing::{error, info, instrument, warn};
use triggered::Listener;

use super::DEFAULT_MOUNT_DIRECTORY;
use crate::{ActionContext, run_container::cache};

type CacheData = HashMap<String, Box<[u8]>>;

#[instrument(skip_all, name = "action_run_judge_compile_execute")]
pub async fn execute(
    handle: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionReportExt> {
    let hash = match config.cache.enabled {
        false => None,
        true => Some(calculate_hash(&ctx.submission_root, config).await?),
    };

    if let Some(hash) = &hash {
        match cache::get(hash.as_ref()) {
            None => {
                info!("Compilation cache miss");
            }
            Some(data) => {
                let data = spawn_blocking(move || {
                    rkyv::from_bytes::<CacheData, rkyv::rancor::Error>(&data)
                })
                .await?
                .context("Error deserializing the data")?;

                for item in &config.saves {
                    if !data.contains_key(item) {
                        bail!("No key found for {item}");
                    }
                }

                info!("Compilation cache hit, reusing files: {}", config.saves.join(", "));

                for (file, data) in data {
                    let mut data = data.as_ref();

                    let target = ctx.submission_root.join(file);
                    let mut writer = BufWriter::new(
                        OpenOptions::new()
                            .create(true)
                            .truncate(true)
                            .write(true)
                            .mode(0o755)
                            .open(&target)
                            .await
                            .with_context(|| {
                                format!("Error opening file to write: {}", target.display())
                            })?,
                    );

                    io::copy_buf(&mut data, &mut writer).await.with_context(|| {
                        format!("Error writing the cached data to file: {}", target.display())
                    })?;
                }

                return Ok(ActionReportExt::Success(ActionSuccessReportExt::RunCompile(
                    ExecutionReport::CacheHit { cache_hit: true },
                )));
            }
        }
    }

    let mount_directory = crate::conf::PATHS.new_temp_directory().await?;
    // XXX: 0o777 is mandatory. The group bit is for rootless case and the others
    // bit is for rootful case.
    fs::set_permissions(&mount_directory, Permissions::from_mode(0o777)).await?;

    let result = async {
        let run_container_config = {
            let mut run_container_config = config.run_container_config.clone();
            run_container_config.cwd = DEFAULT_MOUNT_DIRECTORY.to_owned();

            run_container_config.mounts.push(run_container::MountConfig::Full(runj::MountConfig {
                from: mount_directory.clone(),
                to: DEFAULT_MOUNT_DIRECTORY.to_owned(),
                options: None,
            }));

            run_container_config.mounts.extend(
                config
                    .sources
                    .iter()
                    .map(|file| runj::MountConfig {
                        from: ctx.submission_root.join(&file.from_path),
                        to: DEFAULT_MOUNT_DIRECTORY.join(&file.to_path),
                        options: None,
                    })
                    .map(run_container::MountConfig::Full),
            );

            run_container_config
        };

        let report = crate::run_container::execute(handle, ctx, &run_container_config).await?;

        if matches!(report, ActionReportExt::Success(_)) {
            let mut cache_data: CacheData = Default::default();
            let mut cache_skipped = false;

            for file in &config.saves {
                let source = mount_directory.join(file);
                let target = ctx.submission_root.join(file);
                let metadata = fs::metadata(&source)
                    .await
                    .with_context(|| format!("The file {file} to save does not exist"))?;

                if !metadata.is_file() {
                    bail!("Unknown supported file type: {file}");
                }

                fs::copy(source, &target).await.context("Error copying the file")?;

                if hash.is_none() || cache_skipped {
                    continue;
                }

                if metadata.len() > config.cache.max_allowed_size_mib * 1024 * 1024 {
                    error!("Skipped caching, the size of file {file} exceeds the limit");
                    cache_skipped = true;
                    continue;
                }

                let name = file.clone();
                let mut file = File::open(&target).await.context("Error opening the file")?;
                let mut data = Vec::with_capacity(metadata.len() as usize);

                file.read_to_end(&mut data).await.context("Error reading the file")?;
                cache_data.insert(name, data.into_boxed_slice());
            }

            if let Some(hash) = hash
                && !cache_data.is_empty()
            {
                let data =
                    spawn_blocking(move || rkyv::to_bytes::<rkyv::rancor::Error>(&cache_data))
                        .await??;
                cache::write(hash, Arc::from(data.into_boxed_slice()));
            }
        }

        Ok(report)
    }
    .await;

    if let Err(err) = fs::remove_dir_all(&mount_directory).await {
        warn!(directory = %mount_directory.display(), "Error removing mount directory: {:#}", err)
    }

    result
}

async fn calculate_hash(submission_root: &Path, config: &Config) -> Result<Box<[u8]>> {
    if config.sources.is_empty() && config.saves.is_empty() && config.cache.extra.is_empty() {
        bail!("No sources, saves or cache.extra provided");
    }

    let mut hasher = Sha256::new();

    hasher.update(format!("{}", config.run_container_config.command));

    for item in &config.cache.extra {
        hasher.update(item);
    }

    let mut saves = config.saves.clone();
    saves.sort();

    for item in saves {
        hasher.update(item);
    }

    let mut sources = config.sources.clone();
    sources.sort();

    for item in sources {
        hasher.update(&item.from_path);
        hasher.update(&item.to_path);

        let mut file = File::open(submission_root.join(&item.from_path))
            .await
            .context("Error opening the file")?;

        let metadata = file.metadata().await?;
        let mut data = Vec::with_capacity(metadata.len() as usize);
        file.read_to_end(&mut data).await.context("Error reading the file")?;

        hasher.update(&data);
    }

    Ok(hasher.finalize().to_vec().into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use seele_shared::entities::run_container::{CommandConfig, run_judge::compile::CacheConfig};
    use tokio::fs;

    use super::*;
    use crate::conf::OciImage;

    #[tokio::test]
    async fn test_calculate_hash() {
        let config = Config {
            run_container_config: run_container::Config {
                image: OciImage::from("test"),
                cwd: "/".into(),
                command: CommandConfig::Simple("".to_owned()),
                fd: None,
                paths: None,
                mounts: vec![],
                limits: Default::default(),
            },
            sources: vec!["main.c".try_into().unwrap()],
            saves: vec!["main".to_owned()],
            cache: CacheConfig {
                enabled: false,
                max_allowed_size_mib: 114,
                extra: vec!["foo".to_owned()],
            },
        };

        fs::create_dir("./test").await.unwrap();
        fs::write("./test/main.c", "114514").await.unwrap();
        let hash = super::calculate_hash(Path::new("./test"), &config).await.unwrap();
        fs::remove_dir_all("./test").await.unwrap();

        assert_eq!(
            hash,
            Box::from([
                82, 149, 220, 24, 143, 19, 85, 7, 99, 196, 213, 38, 158, 201, 135, 178, 214, 128,
                38, 78, 70, 25, 170, 85, 181, 110, 238, 161, 33, 239, 43, 21
            ])
        )
    }
}
