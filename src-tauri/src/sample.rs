use crate::{
    files::OpenError,
    meta::{FileMeta, failure},
};
use arrow::record_batch::RecordBatch;
use parquet::arrow::{
    ProjectionMask,
    arrow_reader::{ParquetRecordBatchReaderBuilder, RowSelection, RowSelector},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    sync::atomic::{AtomicBool, Ordering},
};

pub type Result<T> = std::result::Result<T, OpenError>;
pub fn parse_error(e: impl ToString) -> OpenError {
    failure("parse_failed", e)
}
#[derive(Clone, Deserialize)]
pub struct Request {
    pub mode: String,
    pub n: usize,
    pub columns: Vec<usize>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Field {
    pub index: usize,
    pub name: String,
    pub data_type: String,
    pub uuid: bool,
}
#[derive(Debug, Serialize)]
pub struct Preview {
    pub fields: Vec<Field>,
    pub rows: Vec<Vec<Option<String>>>,
    pub total: usize,
    pub offset: usize,
}

pub fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(failure("cancelled", "Task cancelled"))
    } else {
        Ok(())
    }
}
pub fn unchanged(meta: &FileMeta) -> Result<()> {
    let now = std::fs::metadata(&meta.path).map_err(|e| failure("unreadable", e))?;
    if now.len().to_string() != meta.size_bytes || now.modified().ok() != meta.modified {
        return Err(failure(
            "file_changed",
            "Source changed since open; reopen it",
        ));
    }
    Ok(())
}
pub fn builder(meta: &FileMeta) -> Result<ParquetRecordBatchReaderBuilder<File>> {
    unchanged(meta)?;
    let file = File::open(&meta.path).map_err(|e| failure("unreadable", e))?;
    ParquetRecordBatchReaderBuilder::try_new(file).map_err(parse_error)
}
pub fn fields(meta: &FileMeta) -> Result<Vec<Field>> {
    let b = builder(meta)?;
    Ok(b.schema()
        .fields()
        .iter()
        .enumerate()
        .map(|(index, f)| Field {
            index,
            name: f.name().clone(),
            data_type: format!("{}", f.data_type()),
            uuid: matches!(
                b.parquet_schema().root_schema().get_fields()[index]
                    .get_basic_info()
                    .logical_type_ref(),
                Some(parquet::basic::LogicalType::Uuid)
            ),
        })
        .collect())
}
// Prefix lengths for each row group. Selection never duplicates a source row.
pub fn plan(lengths: &[usize], mode: &str, n: usize) -> Result<Vec<usize>> {
    if !(1..=1_000_000).contains(&n) || !matches!(mode, "head" | "uniform") {
        return Err(failure(
            "invalid_request",
            "N must be 1..1000000; mode must be head or uniform",
        ));
    }
    let target = n.min(lengths.iter().sum());
    let mut remaining = target;
    let quota = if mode == "head" {
        n
    } else {
        n.div_ceil(lengths.len().max(1))
    };
    let mut result: Vec<_> = lengths
        .iter()
        .map(|&len| {
            let take = len.min(quota).min(remaining);
            remaining -= take;
            take
        })
        .collect();
    for (take, &len) in result.iter_mut().zip(lengths) {
        let extra = (len - *take).min(remaining);
        *take += extra;
        remaining -= extra;
    }
    Ok(result)
}
pub fn selected(req: &Request, count: usize) -> Result<Vec<usize>> {
    let mut cols = req.columns.clone();
    cols.sort_unstable();
    cols.dedup();
    if cols.is_empty() || cols.last().is_some_and(|&v| v >= count) {
        return Err(failure(
            "invalid_request",
            "Select at least one valid top-level column",
        ));
    }
    Ok(cols)
}
// Batches are small and consumed immediately. Row selection is pushed into the
// reader; decoding stops after the requested range, never after a full RG.
pub fn scan(
    meta: &FileMeta,
    req: &Request,
    offset: usize,
    limit: usize,
    cancel: &AtomicBool,
    mut consume: impl FnMut(&RecordBatch, usize) -> Result<()>,
) -> Result<usize> {
    check(cancel)?;
    let b = builder(meta)?;
    let cols = selected(req, b.schema().fields().len())?;
    let lens: Vec<_> = b
        .metadata()
        .row_groups()
        .iter()
        .map(|g| g.num_rows() as usize)
        .collect();
    let takes = plan(&lens, &req.mode, req.n)?;
    let total: usize = takes.iter().sum();
    let mut skip = offset;
    let mut left = limit.min(total.saturating_sub(offset));
    let mut groups = Vec::new();
    let mut selectors = Vec::new();
    for (i, (&len, &take)) in lens.iter().zip(&takes).enumerate() {
        let before = skip.min(take);
        skip -= before;
        let count = (take - before).min(left);
        left -= count;
        if count > 0 {
            groups.push(i);
            if before > 0 {
                selectors.push(RowSelector::skip(before));
            }
            selectors.push(RowSelector::select(count));
            if len > before + count {
                selectors.push(RowSelector::skip(len - before - count));
            }
        }
    }
    if groups.is_empty() {
        unchanged(meta)?;
        return Ok(total);
    }
    let projection = ProjectionMask::roots(b.parquet_schema(), cols);
    let mut reader = b
        .with_projection(projection)
        .with_row_groups(groups)
        .with_row_selection(RowSelection::from(selectors))
        .with_batch_size(256)
        .build()
        .map_err(parse_error)?;
    let mut done = 0;
    loop {
        check(cancel)?;
        let Some(batch) = reader.next() else {
            break;
        };
        let batch = batch.map_err(parse_error)?;
        consume(&batch, done)?;
        done += batch.num_rows();
    }
    check(cancel)?;
    unchanged(meta)?;
    Ok(total)
}
pub fn preview(
    meta: &FileMeta,
    req: &Request,
    offset: usize,
    limit: usize,
    cancel: &AtomicBool,
) -> Result<Preview> {
    if !(1..=500).contains(&limit) {
        return Err(failure("invalid_request", "Preview limit must be 1..500"));
    }
    let all = fields(meta)?;
    let cols = selected(req, all.len())?;
    let fields: Vec<_> = cols.iter().map(|&i| all[i].clone()).collect();
    let mut rows = Vec::new();
    let mut bytes = 0usize;
    let total = scan(meta, req, offset, limit, cancel, |batch, _| {
        for r in 0..batch.num_rows() {
            check(cancel)?;
            let row: Vec<_> = batch
                .columns()
                .iter()
                .zip(&fields)
                .map(|(a, f)| crate::value::cell(a.as_ref(), r, f.uuid).map(|v| v.text()))
                .collect::<Result<_>>()?;
            bytes += row.iter().flatten().map(String::len).sum::<usize>();
            if bytes > 32 * 1024 * 1024 {
                return Err(failure(
                    "resource_limit",
                    "Preview page exceeds 32 MiB; select fewer columns or fewer rows",
                ));
            }
            rows.push(row);
        }
        Ok(())
    })?;
    Ok(Preview {
        fields,
        rows,
        total,
        offset,
    })
}
