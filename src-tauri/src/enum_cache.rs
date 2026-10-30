use crate::{enums::EnumResult, meta::FileMeta, meta::failure, sample::Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, FileTimes},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::SystemTime,
};

pub const LIMIT: u64 = 50_000_000;
pub const VERSION: u32 = 1;
#[derive(Default, Serialize, Deserialize)]
struct Disk {
    version: u32,
    fingerprint: String,
    entries: BTreeMap<String, EnumResult>,
}
pub struct Cache {
    pub dir: PathBuf,
    gate: Mutex<()>,
    epoch: AtomicU64,
    limit: u64,
}
fn error(e: impl ToString) -> crate::files::OpenError {
    failure("cache_failed", e)
}
pub fn hash(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn fingerprint(meta: &FileMeta, schema: &str) -> String {
    hash(&format!(
        "{}|{}|{:?}|{}|{VERSION}",
        meta.path, meta.size_bytes, meta.modified, schema
    ))
}
pub fn entry_key(root: usize, cap: usize, mode: &str, k: usize) -> String {
    format!("{root}:{cap}:{mode}:{k}")
}
impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self::with_limit(dir, LIMIT)
    }
    pub fn with_limit(dir: PathBuf, limit: u64) -> Self {
        Self {
            dir,
            gate: Mutex::new(()),
            epoch: AtomicU64::new(0),
            limit,
        }
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }
    fn path(&self, path: &str) -> PathBuf {
        self.dir.join(format!("{}.json", hash(path)))
    }
    fn load(&self, path: &Path, fp: &str) -> Result<Disk> {
        if !path.exists() {
            return Ok(Disk {
                version: VERSION,
                fingerprint: fp.into(),
                ..Default::default()
            });
        }
        let size = fs::metadata(path).map_err(error)?.len();
        if size > self.limit {
            return Err(error("Oversized cache entry"));
        }
        let file = File::open(path).map_err(error)?;
        let data: Disk = serde_json::from_reader(std::io::BufReader::new(file)).map_err(error)?;
        if data.version != VERSION || data.fingerprint != fp {
            return Ok(Disk {
                version: VERSION,
                fingerprint: fp.into(),
                ..Default::default()
            });
        }
        if data
            .entries
            .iter()
            .any(|(k, v)| !v.valid_cache() || *k != entry_key(v.column.root, v.cap, &v.mode, v.k))
        {
            return Err(error("Invalid cached counts or result state"));
        }
        Ok(data)
    }
    pub fn read(&self, path: &str, fp: &str) -> Result<BTreeMap<String, EnumResult>> {
        let _guard = self.gate.lock().map_err(error)?;
        let p = self.path(path);
        let data = self.load(&p, fp)?;
        if p.exists() {
            File::options()
                .write(true)
                .open(p)
                .and_then(|f| f.set_times(FileTimes::new().set_modified(SystemTime::now())))
                .map_err(error)?;
        }
        Ok(data.entries)
    }
    pub fn write(
        &self,
        path: &str,
        fp: &str,
        entries: BTreeMap<String, EnumResult>,
        epoch: u64,
    ) -> Result<bool> {
        let _guard = self.gate.lock().map_err(error)?;
        if self.epoch() != epoch {
            return Ok(false);
        }
        fs::create_dir_all(&self.dir).map_err(error)?;
        let p = self.path(path);
        let mut data = self.load(&p, fp).unwrap_or_else(|_| Disk {
            version: VERSION,
            fingerprint: fp.into(),
            ..Default::default()
        });
        data.entries
            .extend(entries.into_iter().filter(|(_, v)| v.valid_cache()));
        let bytes = serde_json::to_vec(&data).map_err(error)?;
        if bytes.len() as u64 > self.limit {
            return Ok(false);
        }
        let mut temp = tempfile::NamedTempFile::new_in(&self.dir).map_err(error)?;
        temp.write_all(&bytes).map_err(error)?;
        temp.as_file().sync_all().map_err(error)?;
        temp.persist(&p).map_err(error)?;
        let mut files = self.files()?;
        files.sort_by_key(|(_, _, t)| *t);
        let mut total: u64 = files.iter().map(|(_, s, _)| s).sum();
        for (file, size, _) in files {
            if total <= self.limit {
                break;
            }
            fs::remove_file(file).map_err(error)?;
            total -= size;
        }
        Ok(true)
    }
    fn files(&self) -> Result<Vec<(PathBuf, u64, SystemTime)>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        fs::read_dir(&self.dir)
            .map_err(error)?
            .filter_map(|entry| match entry {
                Err(e) => Some(Err(error(e))),
                Ok(e) => {
                    let path = e.path();
                    let valid =
                        path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
                            s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
                        }) && path.extension().is_some_and(|s| s == "json");
                    if !valid {
                        return None;
                    }
                    Some(
                        e.metadata()
                            .map(|m| {
                                (
                                    path,
                                    m.len(),
                                    m.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                                )
                            })
                            .map_err(error),
                    )
                }
            })
            .collect()
    }
    pub fn clear(&self) -> Result<()> {
        let _guard = self.gate.lock().map_err(error)?;
        self.epoch.fetch_add(1, Ordering::AcqRel);
        // Only our digest-named JSON files, never a recursive directory deletion.
        for (path, _, _) in self.files()? {
            fs::remove_file(path).map_err(error)?;
        }
        Ok(())
    }
    pub fn size(&self) -> Result<u64> {
        let _guard = self.gate.lock().map_err(error)?;
        Ok(self.files()?.iter().map(|(_, n, _)| n).sum())
    }
}
