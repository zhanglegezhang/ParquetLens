use crate::{meta::failure, sample::Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::Mutex};

pub fn base_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ParquetLens")
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub version: u32,
    pub cap: usize,
    pub sample_n: usize,
    pub time_mode: String,
    pub theme: String,
    pub language: String,
    pub recent: Vec<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            cap: 200,
            sample_n: 1000,
            time_mode: "utc".into(),
            theme: "system".into(),
            language: "zh-CN".into(),
            recent: Vec::new(),
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !(50..=5000).contains(&self.cap)
            || !(1..=1_000_000).contains(&self.sample_n)
            || !matches!(self.time_mode.as_str(), "utc" | "local")
            || !matches!(self.theme.as_str(), "system" | "light" | "dark")
            || self.language != "zh-CN"
            || self.recent.len() > 10
        {
            return Err(failure("invalid_request", "Invalid settings"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
pub struct Snapshot {
    pub preferences: Preferences,
    pub warning: Option<String>,
}
pub struct Settings {
    path: PathBuf,
    inner: Mutex<Snapshot>,
}
impl Settings {
    pub fn new(path: PathBuf) -> Self {
        let (preferences, warning) = if path.exists() {
            match fs::metadata(&path)
                .map(|m| m.len() <= 2 * 1024 * 1024)
                .and_then(|ok| {
                    if ok {
                        fs::read(&path)
                    } else {
                        Err(std::io::Error::other("Settings exceed 2 MiB"))
                    }
                })
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice::<Preferences>(&b).map_err(|e| e.to_string()))
                .and_then(|p| p.validate().map(|_| p).map_err(|e| e.detail))
            {
                Ok(p) => (p, None),
                Err(e) => (
                    Preferences::default(),
                    Some(format!("Settings reset in memory: {e}")),
                ),
            }
        } else {
            (Preferences::default(), None)
        };
        Self {
            path,
            inner: Mutex::new(Snapshot {
                preferences,
                warning,
            }),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn persist(&self, p: &Preferences) -> std::result::Result<(), String> {
        let parent = self.path.parent().ok_or("Missing settings directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        tmp.write_all(&serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        tmp.as_file().sync_all().map_err(|e| e.to_string())?;
        tmp.persist(&self.path).map_err(|e| e.to_string())?;
        Ok(())
    }
    pub fn save(&self, mut p: Preferences) -> Result<Snapshot> {
        p.validate()?;
        let mut s = self.inner.lock().map_err(|e| failure("task_failed", e))?;
        p.recent = s.preferences.recent.clone();
        s.preferences = p;
        s.warning = self.persist(&s.preferences).err();
        Ok(s.clone())
    }
    pub fn recent(&self, path: Option<String>) -> Result<Snapshot> {
        let mut s = self.inner.lock().map_err(|e| failure("task_failed", e))?;
        if let Some(path) = path {
            s.preferences.recent.retain(|p| p != &path);
            s.preferences.recent.insert(0, path);
            s.preferences.recent.truncate(10);
        } else {
            s.preferences.recent.clear();
        }
        s.warning = self.persist(&s.preferences).err();
        Ok(s.clone())
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self::new(base_dir().join("settings.json"))
    }
}
