use std::{
    fs::Permissions,
    os::unix::prelude::PermissionsExt,
    path::PathBuf,
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use duct::{Handle, cmd};
use futures_util::FutureExt;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use seele_config::OciImage;
use tokio::{
    fs::{self, create_dir_all, metadata, remove_dir_all},
    sync::oneshot,
    time::sleep,
};
use tracing::{Span, debug, error, info, instrument, warn};
use triggered::Listener;

use crate::{
    conf,
    shared::{self, cond::CondGroup, runner},
};

static PREPARATION_TASKS: LazyLock<CondGroup<OciImage, Result<(), String>>> = LazyLock::new(|| {
    CondGroup::new(|payload: &OciImage| prepare_image_impl(payload.clone()).boxed())
});

pub async fn prepare_image(abort: Listener, image: OciImage) -> Result<()> {
    match PREPARATION_TASKS.run(image, abort).await {
        None => bail!(shared::ABORTED_MESSAGE),
        Some(Err(err)) => bail!("Error preparing the image: {err:#}"),
        _ => Ok(()),
    }
}

#[instrument]
async fn prepare_image_impl(image: OciImage) -> Result<(), String> {
    pull_image(&image).await.map_err(|err| format!("Error pulling the image: {err:#}"))?;
    unpack_image(&image).await.map_err(|err| format!("Error unpacking the image: {err:#}"))?;
    Ok(())
}

#[instrument(skip_all)]
async fn pull_image(image: &OciImage) -> Result<()> {
    let target_path = get_oci_image_path(image);
    if metadata(&target_path).await.is_ok() {
        debug!(path = %target_path.display(), "The image directory already presents, skip pulling");
        return Ok(());
    }

    let temp_target_path = get_temp_oci_image_path(image);
    {
        if metadata(&temp_target_path).await.is_ok() {
            warn!(path = %temp_target_path.display(),"The temp image directory already exists");
            remove_dir_all(&temp_target_path)
                .await
                .context("Error deleting the existing temp image directory")?;
        }
        create_dir_all(&temp_target_path)
            .await
            .context("Error creating the temp image directory")?;
    }

    // TODO: Should be placed inside submission root
    let skopeo_log_file_path =
        conf::PATHS.temp.join(format!("skopeo-{}.log", nano_id::base62::<12>()));

    let (handle_tx, cancel_tx) = make_timeout_killer(
        1 + conf::CONFIG.worker.action.run_container.pull_image_timeout_seconds,
    );

    info!(path = %temp_target_path.display(), skopeo = conf::CONFIG.paths.skopeo, "Pulling the image using skopeo");
    let success = runner::spawn_blocking({
        let image = image.clone();
        let temp_target_path = temp_target_path.clone();
        let skopeo_log_file_path = skopeo_log_file_path.clone();
        move || {
            let Ok(handle) = cmd!(
                &conf::CONFIG.paths.skopeo,
                "copy",
                &format!("docker://{}/{}:{}", image.registry, image.name, image.tag),
                &format!("oci:{}:{}", temp_target_path.display(), image.tag),
                "--command-timeout",
                &format!(
                    "{}s",
                    conf::CONFIG.worker.action.run_container.pull_image_timeout_seconds
                ),
                "--retry-times",
                "3",
                "--quiet"
            )
            .env("GOMAXPROCS", "1")
            .stdout_path(skopeo_log_file_path)
            .stderr_to_stdout()
            .unchecked()
            .start() else {
                _ = cancel_tx.send(());
                bail!("Error starting umoci process");
            };

            let handle = Arc::new(handle);
            _ = handle_tx.send(handle.clone());

            let result = handle.wait();
            _ = cancel_tx.send(());
            Ok(result?.status.success())
        }
    })
    .await?
    .context("Error running skopeo")?;

    if !success {
        let content = shared::tail(
            fs::File::open(&skopeo_log_file_path).await.context("Error opening log file")?,
            1024,
        )
        .await
        .context("Error reading log file")?;

        _ = fs::remove_file(&skopeo_log_file_path).await;

        bail!(
            "The skopeo process failed, output file {}: {}",
            skopeo_log_file_path.display(),
            String::from_utf8_lossy(&content[..])
        );
    }

    fs::rename(&temp_target_path, target_path)
        .await
        .context("Error moving the image from temp directory")?;

    _ = fs::remove_file(&skopeo_log_file_path).await;

    Ok(())
}

#[instrument(skip_all)]
async fn unpack_image(image: &OciImage) -> Result<()> {
    let unpacked_path = get_unpacked_image_path(image);
    if metadata(&unpacked_path).await.is_ok() {
        debug!(path = %unpacked_path.display(), "The image directory already presents, skip ");
        return Ok(());
    }

    let temp_unpacked_path = get_temp_unpacked_image_path(image);
    {
        if metadata(&temp_unpacked_path).await.is_ok() {
            warn!(path = %temp_unpacked_path.display(), "The temp unpacked directory already exists");
            remove_dir_all(&temp_unpacked_path)
                .await
                .context("Error deleting the existing temp unpacked directory")?;
        }
        create_dir_all(&temp_unpacked_path)
            .await
            .context("Error creating the temp unpacked directory")?;
    }

    // TODO: Should be placed inside submission root
    let umoci_log_file_path =
        conf::PATHS.temp.join(format!("umoci-{}.log", nano_id::base62::<12>()));

    let (handle_tx, cancel_tx) =
        make_timeout_killer(conf::CONFIG.worker.action.run_container.unpack_image_timeout_seconds);

    info!(path = %temp_unpacked_path.display(), umoci = conf::CONFIG.paths.umoci, "Unpacking the image using umoci");
    let success = runner::spawn_blocking({
        let image = image.clone();
        let image_path = get_oci_image_path(&image);
        let temp_unpacked_path = temp_unpacked_path.clone();
        let umoci_log_file_path = umoci_log_file_path.clone();
        move || {
            let Ok(handle) = cmd!(
                &conf::CONFIG.paths.umoci,
                "--log",
                "error",
                "unpack",
                "--rootless",
                "--image",
                &format!("{}:{}", image_path.display(), image.tag),
                &format!("{}", temp_unpacked_path.display()),
            )
            .env("GOMAXPROCS", "1")
            .stdout_path(umoci_log_file_path)
            .stderr_to_stdout()
            .unchecked()
            .start() else {
                _ = cancel_tx.send(());
                bail!("Error starting umoci process");
            };

            let handle = Arc::new(handle);
            _ = handle_tx.send(handle.clone());

            let result = handle.wait();
            _ = cancel_tx.send(());
            Ok(result?.status.success())
        }
    })
    .await?
    .context("Error running umoci")?;

    if !success {
        let content = shared::tail(
            fs::File::open(&umoci_log_file_path).await.context("Error opening log file")?,
            1024,
        )
        .await
        .context("Error reading log file")?;

        _ = fs::remove_file(&umoci_log_file_path).await;

        bail!(
            "The umoci process failed, output file {}: {}",
            umoci_log_file_path.display(),
            String::from_utf8_lossy(&content[..])
        );
    }

    fs::rename(temp_unpacked_path, &unpacked_path)
        .await
        .context("Error moving unpacked image from temp directory")?;

    fs::set_permissions(&unpacked_path, Permissions::from_mode(0o777))
        .await
        .context("Error setting the permission of unpacked directory")?;

    _ = fs::remove_file(&umoci_log_file_path).await;

    Ok(())
}

pub fn make_timeout_killer(
    timeout_seconds: u64,
) -> (oneshot::Sender<Arc<Handle>>, oneshot::Sender<()>) {
    let (handle_tx, handle_rx) = oneshot::channel::<Arc<Handle>>();
    let (cancel_tx, cancel_rx) = oneshot::channel();

    tokio::spawn({
        let span = Span::current();
        async move {
            let wait = handle_rx
                .then(|handle| sleep(Duration::from_secs(timeout_seconds)).map(move |_| handle));
            tokio::select! {
                _ = cancel_rx => (),
                handle = wait => match handle {
                    Err(_) => (),
                    Ok(handle) => {
                        error!(parent: span, "Execution timeout, killing the process");
                        for pid in handle.pids() {
                            _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGKILL);
                        }
                    }
                }
            }
        }
    });

    (handle_tx, cancel_tx)
}

#[inline]
pub fn get_image_path(image: &OciImage) -> PathBuf {
    // Tag name: https://docs.docker.com/engine/reference/commandline/tag/#description
    conf::PATHS.images.join(&image.registry).join(escape_image_name(&image.name)).join(&image.tag)
}

#[inline]
pub fn get_oci_image_path(image: &OciImage) -> PathBuf {
    get_image_path(image).join("oci")
}

#[inline]
pub fn get_temp_oci_image_path(image: &OciImage) -> PathBuf {
    get_image_path(image).join("temp_oci")
}

#[inline]
pub fn get_unpacked_image_path(image: &OciImage) -> PathBuf {
    get_image_path(image).join("unpacked")
}

#[inline]
pub fn get_temp_unpacked_image_path(image: &OciImage) -> PathBuf {
    get_image_path(image).join("temp_unpacked")
}

#[inline]
fn escape_image_name(name: &str) -> String {
    // https://docs.docker.com/registry/spec/api/#overview
    name.replace('/', "_")
}
