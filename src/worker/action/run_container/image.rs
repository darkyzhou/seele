use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use duct::cmd;
use futures_util::FutureExt;
use once_cell::sync::Lazy;
use tokio::{
    fs::{self, create_dir_all, metadata, remove_dir_all},
    task::spawn_blocking,
};
use tracing::{debug, instrument, warn};

use crate::{
    conf,
    shared::{self, cond_group::CondGroup, oci_image::OciImage},
};

static PREPARATION_TASKS: Lazy<CondGroup<OciImage, Result<(), String>>> =
    Lazy::new(|| CondGroup::new(|payload: &OciImage| prepare_image_impl(payload.clone()).boxed()));

pub async fn prepare_image(image: &OciImage) -> Result<()> {
    PREPARATION_TASKS.run(image).await.map_err(|err| anyhow!(err))?;
    Ok(())
}

#[instrument]
async fn prepare_image_impl(image: OciImage) -> Result<(), String> {
    pull_image(&image).await.map_err(|err| format!("Error pulling the image: {err:#}"))?;
    unpack_image(&image).await.map_err(|err| format!("Error unpacking the image: {err:#}"))?;
    Ok(())
}

#[instrument]
async fn pull_image(image: &OciImage) -> Result<()> {
    const PULL_TIMEOUT_SECOND: u64 = 180;

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
        conf::PATHS.temp.join(&format!("skopeo-{}.log", nano_id::base62::<12>()));

    debug!(path = %temp_target_path.display(), skopeo = conf::CONFIG.skopeo_path, "Pulling the image using skopeo");
    let output = spawn_blocking({
        let image = image.clone();
        let temp_target_path = temp_target_path.clone();
        let skopeo_log_file_path = skopeo_log_file_path.clone();
        move || {
            cmd!(
                &conf::CONFIG.skopeo_path,
                "copy",
                &format!("docker://{}/{}:{}", image.registry, image.name, image.tag),
                &format!("oci:{}:{}", temp_target_path.display(), image.tag),
                "--command-timeout",
                &format!("{PULL_TIMEOUT_SECOND}s"),
                "--retry-times",
                "3",
                "--quiet"
            )
            .env("GOMAXPROCS", "1")
            .stdout_path(skopeo_log_file_path)
            .stderr_to_stdout()
            .unchecked()
            .run()
        }
    })
    .await?
    .context("Error running skopeo")?;

    if !output.status.success() {
        let content = shared::tail(
            fs::File::open(&skopeo_log_file_path).await.context("Error opening log file")?,
            1024,
        )
        .await
        .context("Error reading log file")?;
        bail!(
            "The skopeo process failed, output file {}: {}",
            skopeo_log_file_path.display(),
            String::from_utf8_lossy(&content[..])
        );
    }

    fs::rename(&temp_target_path, target_path)
        .await
        .context("Error moving the image from temp directory")?;

    Ok(())
}

#[instrument]
async fn unpack_image(image: &OciImage) -> Result<()> {
    const UNPACK_TIMEOUT_SECOND: u64 = 120;

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
        conf::PATHS.temp.join(&format!("umoci-{}.log", nano_id::base62::<12>()));

    debug!(path = %temp_unpacked_path.display(), umoci = conf::CONFIG.umoci_path, "Unpacking the image using umoci");
    let output = spawn_blocking({
        let image = image.clone();
        let image_path = get_oci_image_path(&image);
        let temp_unpacked_path = temp_unpacked_path.clone();
        let umoci_log_file_path = umoci_log_file_path.clone();
        move || {
            cmd!(
                &conf::CONFIG.umoci_path,
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
            .run()
        }
    })
    .await?
    .context("Error running umoci")?;

    if !output.status.success() {
        let content = shared::tail(
            fs::File::open(&umoci_log_file_path).await.context("Error opening log file")?,
            1024,
        )
        .await
        .context("Error reading log file")?;
        bail!(
            "The umoci process failed, output file {}: {}",
            umoci_log_file_path.display(),
            String::from_utf8_lossy(&content[..])
        );
    }

    fs::rename(temp_unpacked_path, &unpacked_path)
        .await
        .context("Error moving unpacked image from temp directory")?;

    Ok(())
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
