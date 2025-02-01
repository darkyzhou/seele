use std::fmt::Display;

use anyhow::bail;
use serde::{Deserialize, Serialize, de};

pub mod compile;
pub mod run;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MountFile {
    pub from_path: String,
    pub to_path: String,
    pub exec: bool,
}

impl<'de> Deserialize<'de> for MountFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = String::deserialize(deserializer)?;
        str.as_str().try_into().map_err(|err| de::Error::custom(format!("{err:#}")))
    }
}

impl Serialize for MountFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl TryFrom<&str> for MountFile {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(match value.split(':').collect::<Vec<_>>()[..] {
            [from_path] => {
                Self { from_path: from_path.into(), to_path: from_path.into(), exec: false }
            }
            [from_path, "exec"] => {
                Self { from_path: from_path.into(), to_path: from_path.into(), exec: true }
            }
            [from_path, to_path] => {
                Self { from_path: from_path.into(), to_path: to_path.into(), exec: false }
            }
            [from_path, to_path, "exec"] => {
                Self { from_path: from_path.into(), to_path: to_path.into(), exec: true }
            }
            _ => bail!("Unexpected file item: {value}"),
        })
    }
}

impl Display for MountFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}{}", self.from_path, self.to_path, if self.exec { ":exec" } else { "" })
    }
}
