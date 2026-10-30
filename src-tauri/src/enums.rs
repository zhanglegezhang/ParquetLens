use crate::{
    enum_cache::{self, Cache},
    enum_count::{Budget, Counter},
    meta::{FileMeta, failure},
    sample::{self, Result},
};
use arrow::datatypes::{DataType, Field, Schema};
use parquet::arrow::{
    ProjectionMask,
    arrow_reader::{ArrowReaderMetadata, ArrowReaderOptions, ParquetRecordBatchReaderBuilder},
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub root: usize,
    pub leaves: Vec<usize>,
    pub name: String,
    pub data_type: String,
    pub candidate: bool,
    pub nested: bool,
    pub numeric: bool,
    pub uuid: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub value: Option<String>,
    pub count: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumResult {
    pub id: String,
    pub column: Column,
    pub status: String,
    pub exact: bool,
    pub cap: usize,
    pub mode: String,
    pub k: usize,
    pub scanned: String,
    pub scope_rows: String,
    pub file_rows: String,
    pub groups: usize,
    pub scope_groups: usize,
    pub file_groups: usize,
    pub distinct: Option<usize>,
    pub source: String,
    pub values: Vec<Entry>,
    pub message: Option<String>,
}
#[derive(Clone, Deserialize)]
pub struct Options {
    pub columns: Vec<usize>,
    pub cap: usize,
    pub mode: String,
    pub k: usize,
    #[serde(default)]
    pub force: bool,
}
#[derive(Clone, Serialize)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub scanned_groups: usize,
}
pub struct Outcome {
    pub results: Vec<EnumResult>,
    pub warnings: Vec<String>,
    pub cancelled: bool,
}
impl EnumResult {
    pub fn summary(&self) -> Self {
        Self {
            id: self.id.clone(),
            column: self.column.clone(),
            status: self.status.clone(),
            exact: self.exact,
            cap: self.cap,
            mode: self.mode.clone(),
            k: self.k,
            scanned: self.scanned.clone(),
            scope_rows: self.scope_rows.clone(),
            file_rows: self.file_rows.clone(),
            groups: self.groups,
            scope_groups: self.scope_groups,
            file_groups: self.file_groups,
            distinct: self.distinct,
            source: self.source.clone(),
            message: self.message.clone(),
            values: Vec::new(),
        }
    }
    pub fn size(&self) -> usize {
        self.values
            .iter()
            .map(|v| v.value.as_ref().map_or(0, String::len) + v.count.len() + 96)
            .sum::<usize>()
            + 1024
    }
    pub fn valid_cache(&self) -> bool {
        if !(50..=5000).contains(&self.cap)
            || !matches!(self.mode.as_str(), "exact" | "quick")
            || self.k == 0
        {
            return false;
        }
        if self.status == "high" {
            return self.values.is_empty() && self.distinct.is_none();
        }
        if self.status != "complete"
            || self.values.len() > self.cap + 1
            || self.distinct != Some(self.values.iter().filter(|v| v.value.is_some()).count())
            || self.values.iter().filter(|v| v.value.is_none()).count() > 1
        {
            return false;
        }
        let sum = self
            .values
            .iter()
            .try_fold(0u64, |a, v| a.checked_add(v.count.parse::<u64>().ok()?));
        sum.is_some_and(|n| {
            Some(n) == self.scanned.parse().ok() && Some(n) == self.scope_rows.parse().ok()
        })
    }
}
fn base_type(t: &DataType) -> &DataType {
    if let DataType::Dictionary(_, t) = t {
        base_type(t)
    } else {
        t
    }
}
fn numeric(t: &DataType) -> bool {
    matches!(
        base_type(t),
        DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::Int64
            | DataType::UInt8
            | DataType::UInt16
            | DataType::UInt32
            | DataType::UInt64
            | DataType::Float32
            | DataType::Float64
            | DataType::Decimal32(..)
            | DataType::Decimal64(..)
            | DataType::Decimal128(..)
            | DataType::Decimal256(..)
    )
}
pub fn catalog(meta: &FileMeta) -> Result<Vec<Column>> {
    let b = sample::builder(meta)?;
    let fields = sample::fields(meta)?;
    Ok(b.schema().fields().iter().enumerate().map(|(root,f)|{
        let t=base_type(f.data_type());let nested=matches!(t,DataType::List(_)|DataType::LargeList(_)|DataType::FixedSizeList(_,_)|DataType::Struct(_)|DataType::Map(_,_));
        let candidate=matches!(t,DataType::Null|DataType::Boolean|DataType::Int8|DataType::Int16|DataType::Int32|DataType::Int64|DataType::UInt8|DataType::UInt16|DataType::UInt32|DataType::UInt64|DataType::Utf8|DataType::LargeUtf8|DataType::Utf8View|DataType::Date32|DataType::Date64)
            || matches!(t,DataType::Decimal32(p,_)|DataType::Decimal64(p,_)|DataType::Decimal128(p,_)|DataType::Decimal256(p,_) if *p<=15);
        Column{root,leaves:(0..b.parquet_schema().num_columns()).filter(|&i|b.parquet_schema().get_column_root_idx(i)==root).collect(),name:f.name().clone(),data_type:t.to_string(),candidate,nested,numeric:numeric(t),uuid:fields[root].uuid}
    }).collect())
}
struct Slot {
    column: Column,
    stop: AtomicBool,
    counter: Mutex<Counter>,
}
fn blank(column: Column, opt: &Options, meta: &FileMeta, rows: u64, groups: usize) -> EnumResult {
    EnumResult {
        id: String::new(),
        column,
        status: "complete".into(),
        exact: groups == meta.row_groups,
        cap: opt.cap,
        mode: opt.mode.clone(),
        k: opt.k,
        scanned: "0".into(),
        scope_rows: rows.to_string(),
        file_rows: meta.num_rows.clone(),
        groups: 0,
        scope_groups: groups,
        file_groups: meta.row_groups,
        distinct: Some(0),
        source: "scan".into(),
        values: Vec::new(),
        message: None,
    }
}
pub fn analyze(
    meta: &FileMeta,
    opt: &Options,
    cancel: &AtomicBool,
    cache: &Cache,
    notify: impl Fn(&EnumResult) + Sync,
    progress: impl Fn(Progress) + Sync,
) -> Result<Outcome> {
    if !(50..=5000).contains(&opt.cap)
        || !matches!(opt.mode.as_str(), "exact" | "quick")
        || opt.k == 0
    {
        return Err(failure(
            "invalid_request",
            "cap 50..5000; mode exact/quick; K >= 1",
        ));
    }
    sample::check(cancel)?;
    let columns = catalog(meta)?;
    let mut chosen = opt.columns.clone();
    chosen.sort_unstable();
    chosen.dedup();
    if chosen.is_empty() || chosen.iter().any(|&c| c >= columns.len()) {
        return Err(failure("invalid_request", "Select valid top-level columns"));
    }
    let builder = sample::builder(meta)?;
    let metadata = builder.metadata().clone();
    let groups = if opt.mode == "quick" {
        opt.k.min(meta.row_groups)
    } else {
        meta.row_groups
    };
    let rows: u64 = metadata.row_groups()[..groups]
        .iter()
        .map(|g| g.num_rows() as u64)
        .sum();
    let fp = enum_cache::fingerprint(meta, &format!("{:?}", builder.schema()));
    let epoch = cache.epoch();
    let mut warnings = Vec::new();
    let cached = if opt.force {
        BTreeMap::new()
    } else {
        match cache.read(&meta.path, &fp) {
            Ok(v) => v,
            Err(e) => {
                warnings.push(e.detail);
                BTreeMap::new()
            }
        }
    };
    let budget = Arc::new(Budget::default());
    let mut slots = Vec::new();
    let mut results = Vec::new();
    for root in chosen {
        let column = columns[root].clone();
        if cancel.load(Ordering::Relaxed) {
            let mut r = blank(column, opt, meta, rows, groups);
            r.status = "cancelled".into();
            r.distinct = None;
            notify(&r);
            results.push(r);
            continue;
        }
        let key = enum_cache::entry_key(root, opt.cap, &opt.mode, opt.k);
        if let Some(r) = cached.get(&key)
            && r.column.name == column.name
            && r.column.data_type == column.data_type
        {
            let mut r = r.clone();
            r.source = "cache".into();
            notify(&r);
            results.push(r);
            continue;
        }
        let mut r = blank(column.clone(), opt, meta, rows, groups);
        let constant = if !column.nested && column.leaves.len() == 1 {
            meta.columns.get(column.leaves[0]).filter(|c| {
                c.stats_complete
                    && !c.min_max_truncated
                    && c.null_count.as_deref() == Some("0")
                    && c.min.is_some()
                    && c.min == c.max
                    && !column.data_type.starts_with("Float")
                    && !column.data_type.starts_with("Timestamp")
            })
        } else {
            None
        };
        if rows == 0 || constant.is_some() {
            r.source = "footer".into();
            r.scanned = rows.to_string();
            r.groups = groups;
            if rows > 0 {
                r.values.push(Entry {
                    value: constant.unwrap().min.clone(),
                    count: rows.to_string(),
                });
                r.distinct = Some(1);
            }
            notify(&r);
            results.push(r);
            continue;
        }
        slots.push(Slot {
            column,
            stop: AtomicBool::new(false),
            counter: Mutex::new(Counter::new(opt.cap, budget.clone())),
        });
    }
    if slots.is_empty() {
        sample::unchanged(meta)?;
        let entries = results
            .iter()
            .filter(|r| r.source != "cache" && r.valid_cache())
            .map(|r| {
                (
                    enum_cache::entry_key(r.column.root, opt.cap, &opt.mode, opt.k),
                    r.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        if !entries.is_empty() {
            match cache.write(&meta.path, &fp, entries, epoch) {
                Ok(false) => warnings
                    .push("Cache skipped: cleared during task or size budget exceeded".into()),
                Err(e) => warnings.push(e.detail),
                _ => {}
            }
        }
        return Ok(Outcome {
            results,
            warnings,
            cancelled: cancel.load(Ordering::Relaxed),
        });
    }
    // Keep byte-array dictionaries where supported. IDs are local to a reader;
    // counters always merge the underlying values, not dictionary IDs.
    let schema = Arc::new(Schema::new_with_metadata(
        builder
            .schema()
            .fields()
            .iter()
            .map(|f| {
                if matches!(
                    f.data_type(),
                    DataType::Utf8 | DataType::LargeUtf8 | DataType::Binary | DataType::LargeBinary
                ) {
                    Arc::new(
                        Field::new(
                            f.name(),
                            DataType::Dictionary(
                                Box::new(DataType::Int32),
                                Box::new(f.data_type().clone()),
                            ),
                            f.is_nullable(),
                        )
                        .with_metadata(f.metadata().clone()),
                    )
                } else {
                    f.clone()
                }
            })
            .collect::<Vec<_>>(),
        builder.schema().metadata().clone(),
    ));
    let file = File::open(&meta.path).map_err(sample::parse_error)?;
    let reader_meta =
        ArrowReaderMetadata::load(&file, ArrowReaderOptions::new().with_schema(schema))
            .map_err(sample::parse_error)?;
    let max_bytes = (0..groups)
        .map(|g| {
            slots
                .iter()
                .flat_map(|s| &s.column.leaves)
                .map(|&l| metadata.row_group(g).column(l).compressed_size().max(0) as usize)
                .sum::<usize>()
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let threads = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(8)
        .min((256 * 1024 * 1024 / max_bytes).max(1));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(sample::parse_error)?;
    let done = AtomicUsize::new(0);
    let scanned_groups = AtomicUsize::new(0);
    let fatal = Mutex::new(None);
    let failed = AtomicBool::new(false);
    pool.install(|| {
        (0..groups).into_par_iter().for_each(|group| {
            if cancel.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed) {
                return;
            }
            let work = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                let group_rows = metadata.row_group(group).num_rows() as usize;
                let mut offset = 0;
                let mut visited = false;
                while offset < group_rows {
                    sample::check(cancel)?;
                    let active: Vec<_> = slots
                        .iter()
                        .enumerate()
                        .filter(|(_, s)| !s.stop.load(Ordering::Relaxed))
                        .map(|(i, _)| i)
                        .collect();
                    if active.is_empty() {
                        break;
                    }
                    let compressed: usize = active
                        .iter()
                        .flat_map(|&i| &slots[i].column.leaves)
                        .map(|&l| {
                            metadata.row_group(group).column(l).compressed_size().max(0) as usize
                        })
                        .sum();
                    if compressed > 256 * 1024 * 1024 {
                        for &i in &active {
                            let s = &slots[i];
                            s.counter.lock().unwrap().stop("resource");
                            s.stop.store(true, Ordering::Relaxed);
                        }
                        break;
                    }
                    let uncompressed: usize = active
                        .iter()
                        .flat_map(|&i| &slots[i].column.leaves)
                        .map(|&l| {
                            metadata
                                .row_group(group)
                                .column(l)
                                .uncompressed_size()
                                .max(0) as usize
                        })
                        .sum();
                    let batch_size = (8 * 1024 * 1024 / (uncompressed / group_rows.max(1)).max(1))
                        .clamp(256, 65536);
                    let projection = ProjectionMask::roots(
                        metadata.file_metadata().schema_descr(),
                        active.iter().map(|&i| slots[i].column.root),
                    );
                    let mut reader = ParquetRecordBatchReaderBuilder::new_with_metadata(
                        File::open(&meta.path).map_err(sample::parse_error)?,
                        reader_meta.clone(),
                    )
                    .with_projection(projection)
                    .with_row_groups(vec![group])
                    .with_offset(offset)
                    .with_batch_size(batch_size)
                    .build()
                    .map_err(sample::parse_error)?;
                    if !visited {
                        scanned_groups.fetch_add(1, Ordering::Relaxed);
                        visited = true;
                    }
                    loop {
                        sample::check(cancel)?;
                        if failed.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        let Some(batch) = reader.next() else {
                            if offset < group_rows {
                                return Err(failure("parse_failed", "Unexpected end of row group"));
                            }
                            break;
                        };
                        let batch = batch.map_err(sample::parse_error)?;
                        offset += batch.num_rows();
                        for (position, &i) in active.iter().enumerate() {
                            let slot = &slots[i];
                            if slot.stop.load(Ordering::Relaxed) {
                                continue;
                            }
                            let mut counter = slot.counter.lock().unwrap();
                            if counter.status != "running" {
                                continue;
                            }
                            counter.add(
                                batch.column(position).as_ref(),
                                slot.column.uuid,
                                cancel,
                            )?;
                            if counter.status != "running" {
                                slot.stop.store(true, Ordering::Relaxed);
                                let mut r = blank(slot.column.clone(), opt, meta, rows, groups);
                                r.status = counter.status.into();
                                r.scanned = counter.scanned.to_string();
                                r.distinct = None;
                                r.groups = counter.groups;
                                notify(&r);
                            }
                        }
                        if active
                            .iter()
                            .any(|&i| slots[i].stop.load(Ordering::Relaxed))
                        {
                            break;
                        }
                    }
                }
                if visited {
                    for s in &slots {
                        let mut c = s.counter.lock().unwrap();
                        if c.status == "running" {
                            c.groups += 1;
                        }
                    }
                }
                Ok(())
            }))
            .unwrap_or_else(|_| Err(failure("task_failed", "Enumeration worker panicked")));
            if let Err(e) = work
                && e.code != "cancelled"
            {
                failed.store(true, Ordering::Relaxed);
                let mut f = fatal.lock().unwrap();
                if f.is_none() {
                    *f = Some(e);
                }
            }
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            progress(Progress {
                done: n,
                total: groups,
                scanned_groups: scanned_groups.load(Ordering::Relaxed),
            });
        })
    });
    if let Some(e) = fatal.into_inner().unwrap() {
        return Err(e);
    }
    sample::unchanged(meta)?;
    let cancelled = cancel.load(Ordering::Relaxed);
    for slot in slots {
        let mut counter = slot
            .counter
            .into_inner()
            .map_err(|_| failure("task_failed", "Enumeration counter poisoned"))?;
        let mut r = blank(slot.column, opt, meta, rows, groups);
        r.scanned = counter.scanned.to_string();
        r.groups = counter.groups;
        if counter.status == "running" && (!cancelled || counter.scanned == rows) {
            if counter.scanned != rows {
                return Err(failure("parse_failed", "Enumeration row count mismatch"));
            }
            r.distinct = Some(counter.values.len());
            r.values = counter
                .values
                .drain(..)
                .map(|(n, v)| Entry {
                    value: Some(v),
                    count: n.to_string(),
                })
                .collect();
            if counter.nulls > 0 {
                r.values.push(Entry {
                    value: None,
                    count: counter.nulls.to_string(),
                });
            }
        } else {
            r.status = if counter.status == "running" {
                "cancelled"
            } else {
                counter.status
            }
            .into();
            r.distinct = None;
        }
        notify(&r);
        results.push(r);
    }
    let entries = results
        .iter()
        .filter(|r| r.source != "cache" && r.valid_cache())
        .map(|r| {
            (
                enum_cache::entry_key(r.column.root, opt.cap, &opt.mode, opt.k),
                r.clone(),
            )
        })
        .collect();
    match cache.write(&meta.path, &fp, entries, epoch) {
        Ok(false) => {
            warnings.push("Cache skipped: cleared during task or size budget exceeded".into())
        }
        Err(e) => warnings.push(e.detail),
        _ => {}
    }
    Ok(Outcome {
        results,
        warnings,
        cancelled,
    })
}
