use anyhow::Context;
use std::path::PathBuf;

mod image;

pub async fn run_container() -> anyhow::Result<()> {
    // let manager = libcgroups::common::create_cgroup_manager("seele", true, "test")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_run_container() {}
}
