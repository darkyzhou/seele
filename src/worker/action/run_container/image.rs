use crate::conf;
use crate::shared::cond_group::CondGroup;
use crate::shared::oci_image::OciImage;
use anyhow::{anyhow, bail, Context};
use futures_util::FutureExt;
use once_cell::sync::Lazy;
use std::path::PathBuf;
use std::time::Duration;
use tokio::process::Command;

macro_rules! collect_output {
    ($output:expr) => {
        String::from_utf8_lossy(
            &$output
                .stdout
                .into_iter()
                .chain($output.stderr.into_iter())
                .take(200)
                .collect::<Vec<_>>(),
        )
    };
}

static PREPARATION_TASKS: Lazy<CondGroup<OciImage, Result<String, String>>> =
    Lazy::new(|| CondGroup::new(|image: &OciImage| prepare_image_impl(image.clone()).boxed()));

pub async fn prepare_image(image: &OciImage) -> anyhow::Result<String> {
    PREPARATION_TASKS.run(image).await.map_err(|msg| anyhow!(msg))
}

async fn prepare_image_impl(image: OciImage) -> Result<String, String> {
    pull_image(&image).await.map_err(|err| format!("Error pulling the image: {:#?}", err))?;

    let path = unpack_image(&image)
        .await
        .map_err(|err| format!("Error unpacking the image: {:#?}", err))?;
    Ok(path)
}

static OCI_IMAGES_PATH: Lazy<PathBuf> =
    Lazy::new(|| [&conf::CONFIG.root_path, "images"].iter().collect());

async fn pull_image(image: &OciImage) -> anyhow::Result<()> {
    const PULL_TIMEOUT_SECONDS: u64 = 30;

    use std::fs::canonicalize;
    use tokio::fs::metadata;
    use tokio::time::timeout;

    let path = canonicalize(get_image_path(image).join("oci"))?;

    // TODO: check the integrity
    if metadata(&path).await.is_ok() {
        return Ok(());
    }

    let output = timeout(
        Duration::from_secs(PULL_TIMEOUT_SECONDS),
        Command::new("skopeo")
            .args([
                "copy",
                &format!("docker://{}/{}:{}", image.registry, image.name, image.tag),
                &format!("oci:{}:{}", path.display(), image.tag),
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
        bail!("The skopeo process failed with the following output: {}", collect_output!(output));
    }
    Ok(())
}

async fn unpack_image(image: &OciImage) -> anyhow::Result<String> {
    const UNPACK_TIMEOUT_SECONDS: u64 = 30;

    use std::fs::canonicalize;
    use tokio::fs::metadata;
    use tokio::time::timeout;

    let image_path = canonicalize(get_image_path(image).join("oci"))?;
    let unpacked_path = canonicalize(get_image_path(image).join("unpacked"))?;

    // TODO: check the integrity
    if metadata(&unpacked_path).await.is_err() {
        let output = timeout(
            Duration::from_secs(UNPACK_TIMEOUT_SECONDS),
            Command::new("umoci")
                .args([
                    "--log",
                    "error",
                    "unpack",
                    "--rootless",
                    "--image",
                    &format!("{}:{}", image_path.display(), image.tag),
                    &format!("{}", unpacked_path.display()),
                ])
                .output(),
        )
        .await
        .context("Error executing the umoci process")?
        .context("The umoci process took too long to finish")?;
        if !output.status.success() {
            bail!(
                "The umoci process failed with the following output: {}",
                collect_output!(output)
            );
        }
    }

    let rootfs_path = unpacked_path
        .join("rootfs")
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow!("Error resolving the rootfs path"))?;
    Ok(rootfs_path)
}

fn get_image_path(image: &OciImage) -> PathBuf {
    OCI_IMAGES_PATH.join(&image.registry).join(escape_image_name(&image.name))
}

fn escape_image_name(name: &str) -> String {
    // https://docs.docker.com/registry/spec/api/#overview
    name.replace('/', "_")
}
