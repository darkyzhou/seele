use crate::{
    conf,
    shared::{self, cond_group::CondGroup, oci_image::OciImage},
};
use anyhow::{anyhow, bail, Context};
use futures_util::FutureExt;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, instrument, warn};

static PREPARATION_TASKS: Lazy<CondGroup<OciImage, Result<String, String>>> =
    Lazy::new(|| CondGroup::new(|payload: &OciImage| prepare_image_impl(payload.clone()).boxed()));

pub async fn prepare_image(image: &OciImage) -> anyhow::Result<String> {
    PREPARATION_TASKS.run(image).await.map_err(|err| anyhow!(err))
}

#[instrument]
async fn prepare_image_impl(image: OciImage) -> Result<String, String> {
    pull_image(&image).await.map_err(|err| format!("Error pulling the image: {err:#}"))?;

    let path =
        unpack_image(&image).await.map_err(|err| format!("Error unpacking the image: {err:#}"))?;
    Ok(path)
}

#[instrument]
async fn pull_image(image: &OciImage) -> anyhow::Result<()> {
    const PULL_TIMEOUT_SECOND: u64 = 180;

    let target_path = get_oci_image_path(image);
    if fs::metadata(&target_path).await.is_ok() {
        debug!(path = %target_path.display(), "The image directory already presents, skip pulling");
        return Ok(());
    }

    let temp_target_path = get_temp_oci_image_path(image);
    if fs::metadata(&temp_target_path).await.is_ok() {
        warn!(path = %temp_target_path.display(),"The temp image directory already exists");
        fs::remove_dir_all(&temp_target_path)
            .await
            .context("Error deleting the existing temp image directory")?;
    }
    fs::create_dir_all(&temp_target_path)
        .await
        .context("Error creating the temp image directory")?;

    debug!(path = %temp_target_path.display(), skopeo = conf::CONFIG.skopeo_path, "Pulling the image using skopeo");
    let output = timeout(
        Duration::from_secs(PULL_TIMEOUT_SECOND + 3),
        Command::new(&conf::CONFIG.skopeo_path)
            .args([
                "copy",
                &format!("docker://{}/{}:{}", image.registry, image.name, image.tag),
                &format!("oci:{}:{}", temp_target_path.display(), image.tag),
                "--command-timeout",
                &format!("{PULL_TIMEOUT_SECOND}s"),
                "--retry-times",
                "3",
                "--quiet",
            ])
            .output(),
    )
    .await
    .context("Error executing the skopeo process")?
    .context("The skopeo process took too long to finish")?;

    if !output.status.success() {
        bail!(
            "The skopeo process failed with the following output: {}",
            shared::collect_output(&output)
        );
    }

    fs::rename(temp_target_path, target_path)
        .await
        .context("Error moving the image from temp directory")?;

    Ok(())
}

#[instrument]
async fn unpack_image(image: &OciImage) -> anyhow::Result<String> {
    const UNPACK_TIMEOUT_SECOND: u64 = 120;

    let image_path = get_oci_image_path(image);
    let unpacked_path = get_unpacked_image_path(image);
    if fs::metadata(&unpacked_path).await.is_err() {
        let temp_unpacked_path = get_temp_unpacked_image_path(image);
        if fs::metadata(&temp_unpacked_path).await.is_ok() {
            warn!(path = %temp_unpacked_path.display(),"The temp unpacked directory already exists");
            fs::remove_dir_all(&temp_unpacked_path)
                .await
                .context("Error deleting the existing temp unpacked directory")?;
        }
        fs::create_dir_all(&temp_unpacked_path)
            .await
            .context("Error creating the temp unpacked directory")?;

        debug!(path = %temp_unpacked_path.display(), umoci = conf::CONFIG.umoci_path, "Unpacking the image using umoci");
        let output = timeout(
            Duration::from_secs(UNPACK_TIMEOUT_SECOND),
            Command::new(&conf::CONFIG.umoci_path)
                .args([
                    "--log",
                    "error",
                    "unpack",
                    "--rootless",
                    "--image",
                    &format!("{}:{}", image_path.display(), image.tag),
                    &format!("{}", temp_unpacked_path.display()),
                ])
                .output(),
        )
        .await
        .context("Error executing the umoci process")?
        .context("The umoci process took too long to finish")?;

        if !output.status.success() {
            bail!(
                "The umoci process failed with the following output: {}",
                shared::collect_output(&output)
            );
        }

        fs::rename(temp_unpacked_path, &unpacked_path)
            .await
            .context("Error moving unpacked image from temp directory")?;
    }

    unpacked_path
        .join("rootfs")
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow!("Error resolving the rootfs path"))
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
