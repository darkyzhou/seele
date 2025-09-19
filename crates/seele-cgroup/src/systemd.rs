use std::{path::PathBuf, process, time::Duration};

use anyhow::{Context, Result, bail};
use dbus::{
    arg::{RefArg, Variant},
    blocking::{Connection, Proxy},
};
use libcgroups::common::DEFAULT_CGROUP_ROOT;

use super::systemd_api::OrgFreedesktopSystemd1Manager;

const PARENT_SLICE: &str = "user.slice";
const SEELE_SCOPE: &str = "seele.scope";

pub fn create_and_enter_cgroup() -> Result<PathBuf> {
    let connection = Connection::new_session().context("Error connecting systemd session bus")?;
    let proxy = create_proxy(&connection);

    let version = systemd_version(&proxy)?;
    if version <= 243 {
        bail!("Seele requires systemd version being greater than 243");
    }

    start_transient_unit(&proxy)?;

    let cgroup_path = proxy.control_group().context("Error getting systemd cgroup path")?;
    Ok([DEFAULT_CGROUP_ROOT, cgroup_path.trim_start_matches('/'), PARENT_SLICE, SEELE_SCOPE]
        .into_iter()
        .collect())
}

fn create_proxy(connection: &Connection) -> Proxy<'_, &Connection> {
    connection.with_proxy(
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        Duration::from_millis(5000),
    )
}

fn start_transient_unit(proxy: &Proxy<&Connection>) -> Result<()> {
    let properties: Vec<(&str, Variant<Box<dyn RefArg>>)> = vec![
        (
            "Description",
            Variant(Box::new("Seele, a modern cloud-native online judge backend".to_string())),
        ),
        ("Delegate", Variant(Box::new(true))),
        ("Slice", Variant(Box::new(PARENT_SLICE.to_string()))),
        ("DefaultDependencies", Variant(Box::new(false))),
        ("PIDs", Variant(Box::new(vec![process::id()]))),
    ];
    proxy
        .start_transient_unit(SEELE_SCOPE, "replace", properties, vec![])
        .context("Error starting transient unit")?;
    Ok(())
}

fn systemd_version(proxy: &Proxy<&Connection>) -> Result<u32> {
    proxy
        .version()
        .context("Error requesting systemd dbus")?
        .chars()
        .skip_while(|c| c.is_alphabetic())
        .take_while(|c| c.is_numeric())
        .collect::<String>()
        .parse::<u32>()
        .context("Error parsing systemd version")
}
