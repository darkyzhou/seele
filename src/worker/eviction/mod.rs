use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    fmt::Debug,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{Mutex, MutexGuard},
    time::sleep,
};
use tracing::{debug, error, info, instrument};

use crate::conf;

pub struct EvictionManager {
    name: String,
    interval: Duration,
    ttl: chrono::Duration,
    capacity: usize,
    state: Mutex<EvictionState>,
}

impl Debug for EvictionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EvictionManager")
            .field("name", &self.name)
            .field("interval", &self.interval)
            .field("ttl", &self.ttl)
            .field("capacity", &self.capacity)
            .finish()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct EvictionState {
    items: BinaryHeap<Reverse<DateTime<Utc>>>,
    time_to_data_map: HashMap<DateTime<Utc>, Vec<PathBuf>>,

    #[serde(skip_serializing, skip_deserializing)]
    preserve_data: HashSet<PathBuf>,
}

impl EvictionManager {
    pub async fn new(
        name: String,
        interval: Duration,
        ttl: Duration,
        capacity: usize,
        state_file: Option<File>,
    ) -> anyhow::Result<Self> {
        let manager = Self {
            name,
            interval,
            ttl: chrono::Duration::from_std(ttl)?,
            capacity,
            state: Default::default(),
        };

        if let Some(mut file) = state_file {
            let mut data = vec![];
            file.read_to_end(&mut data).await?;
            manager.load_states(&data).await?;
        }

        Ok(manager)
    }

    #[instrument]
    pub async fn run_loop(&self) {
        loop {
            info!("Start doing cleaning");
            if let Err(err) = self.clean().await {
                error!("Error doing cleaning: {:#}", err);
            }

            sleep(self.interval).await;
        }
    }

    #[instrument(level = "debug")]
    pub async fn visit_once(&self, data: &PathBuf) {
        let state = self.state.lock().await;
        debug!("Visit once");
        self.do_visit(state, data).await;
    }

    #[instrument(level = "debug")]
    pub async fn visit_enter(&self, data: &PathBuf) {
        let mut state = self.state.lock().await;
        debug!("Visit enter");
        state.preserve_data.insert(data.clone());
        self.do_visit(state, data).await;
    }

    #[instrument(level = "debug")]
    pub async fn visit_leave(&self, data: &PathBuf) {
        let mut state = self.state.lock().await;
        debug!("Visit leave");
        state.preserve_data.remove(data);
    }

    pub async fn load_states(&self, data: &[u8]) -> anyhow::Result<usize> {
        let recovered: EvictionState = ciborium::de::from_reader(data)?;
        let mut state = self.state.lock().await;
        state.items = recovered.items;
        state.preserve_data = recovered.preserve_data;
        state.time_to_data_map = recovered.time_to_data_map;
        Ok(state.items.len())
    }

    pub async fn save_states(&self, writer: &mut Vec<u8>) -> anyhow::Result<()> {
        let state = self.state.lock().await;
        Ok(ciborium::ser::into_writer(&*state, writer)?)
    }

    async fn do_visit<'a>(&self, mut state: MutexGuard<'a, EvictionState>, data: &Path) {
        let now = Utc::now();

        state.items.push(Reverse(now));

        match state.time_to_data_map.get_mut(&now) {
            None => {
                state.time_to_data_map.insert(now, vec![data.into()]);
            }
            Some(vec) => {
                vec.push(data.into());
            }
        }
    }

    async fn clean(&self) -> anyhow::Result<()> {
        let now = Reverse(Utc::now());
        let mut state = self.state.lock().await;

        let evicted_items = {
            let mut eviected_items = vec![];
            let mut preserved_times = vec![];

            loop {
                match state.items.peek() {
                    None => break,
                    Some(time) => {
                        let time = time.0;

                        let within_ttl = {
                            let duration = now.0.signed_duration_since(time);
                            duration < self.ttl
                        };
                        let not_overflow = state.items.len() <= self.capacity;
                        if within_ttl && not_overflow {
                            break;
                        }

                        match state.time_to_data_map.remove(&time) {
                            None => bail!("Missing time_to_data record for {:?}", time),
                            Some(data) => {
                                let (preserved, eviected): (Vec<_>, Vec<_>) = data
                                    .into_iter()
                                    .partition(|item| state.preserve_data.contains(item));
                                eviected_items.extend(eviected);

                                if !preserved.is_empty() {
                                    debug!("Preserving items: {:?}", preserved);
                                    state.time_to_data_map.insert(time, preserved);
                                    preserved_times.push(time);
                                }

                                let _ = state.items.pop();
                            }
                        }
                    }
                }
            }

            for time in preserved_times {
                state.items.push(Reverse(time));
            }

            eviected_items
        };

        debug!("Evicting files: {:?}", evicted_items);
        futures_util::future::join_all(
            evicted_items
                .into_iter()
                .map(|path| conf::CONFIG.root_path.join(path))
                .map(|path| async move { do_evict(&path).await }),
        )
        .await
        .into_iter()
        .collect()
    }
}

async fn do_evict(path: &Path) -> anyhow::Result<()> {
    use tokio::fs;

    let target = &conf::PATHS.evicted.join({
        let random = nano_id::base62::<8>();
        let name = path
            .file_name()
            .ok_or_else(|| anyhow!("Invalid file, expected file name"))?
            .to_str()
            .ok_or_else(|| anyhow!("Unexpected file name"))?;
        format!("{random}_{name}")
    });
    fs::rename(path, target).await?;

    if fs::metadata(target).await?.is_file() {
        fs::remove_file(target).await?;
    } else {
        fs::remove_dir_all(target).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc, time::Duration};

    use tokio::time::sleep;

    #[tokio::test]
    async fn test_eviction_capacity() {
        let manager = Arc::new(
            super::EvictionManager::new(
                "test".to_string(),
                Duration::from_millis(300),
                Duration::from_secs(100),
                2,
                None,
            )
            .await
            .unwrap(),
        );

        manager.visit_once(&PathBuf::from("1")).await;
        manager.visit_once(&PathBuf::from("2")).await;
        manager.visit_once(&PathBuf::from("3")).await;

        tokio::spawn({
            let manager = manager.clone();
            async move {
                manager.run_loop().await;
            }
        });

        sleep(Duration::from_millis(500)).await;

        let mut state = manager.state.lock().await;
        assert_eq!(state.items.len(), 2);
        assert_eq!(state.time_to_data_map.len(), 2);

        let top = &state.items.pop().unwrap().0;
        assert_eq!(state.time_to_data_map.remove(top), Some(vec![PathBuf::from("2")]));
        let top = &state.items.pop().unwrap().0;
        assert_eq!(state.time_to_data_map.remove(top), Some(vec![PathBuf::from("3")]));
    }

    #[tokio::test]
    async fn test_eviction_ttl() {
        let manager = Arc::new(
            super::EvictionManager::new(
                "test".to_string(),
                Duration::from_millis(100),
                Duration::from_millis(200),
                10,
                None,
            )
            .await
            .unwrap(),
        );

        manager.visit_once(&PathBuf::from("1")).await;
        manager.visit_once(&PathBuf::from("2")).await;
        manager.visit_once(&PathBuf::from("3")).await;

        tokio::spawn({
            let manager = manager.clone();
            async move {
                manager.run_loop().await;
            }
        });

        sleep(Duration::from_millis(500)).await;

        let state = manager.state.lock().await;
        assert_eq!(state.items.len(), 0);
        assert_eq!(state.time_to_data_map.len(), 0);
    }

    #[tokio::test]
    async fn test_visit_enter_leave() {
        let manager = Arc::new(
            super::EvictionManager::new(
                "test".to_string(),
                Duration::from_millis(300),
                Duration::from_secs(100),
                2,
                None,
            )
            .await
            .unwrap(),
        );

        manager.visit_enter(&PathBuf::from("1")).await;
        manager.visit_enter(&PathBuf::from("2")).await;
        manager.visit_once(&PathBuf::from("3")).await;
        manager.visit_leave(&PathBuf::from("2")).await;

        tokio::spawn({
            let manager = manager.clone();
            async move {
                manager.run_loop().await;
            }
        });

        sleep(Duration::from_millis(500)).await;

        let mut state = manager.state.lock().await;
        assert_eq!(state.items.len(), 3);
        assert_eq!(state.time_to_data_map.len(), 3);

        let top = &state.items.pop().unwrap().0;
        assert_eq!(state.time_to_data_map.remove(top), Some(vec![PathBuf::from("1")]));
        let top = &state.items.pop().unwrap().0;
        assert_eq!(state.time_to_data_map.remove(top), Some(vec![PathBuf::from("2")]));
        let top = &state.items.pop().unwrap().0;
        assert_eq!(state.time_to_data_map.remove(top), Some(vec![PathBuf::from("3")]));
    }
}
