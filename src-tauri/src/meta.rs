use crate::files::OpenError;
use crate::format::{self, Kind, Scalar};
use parquet::basic::SortOrder;
use parquet::file::metadata::{ParquetMetaData, ParquetMetaDataReader};
use serde::Serialize;
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, Clone, Serialize)]
pub struct ColumnInfo {
    pub index: usize,
    pub path: String,
    pub logical_type: String,
    pub physical_type: String,
    pub nullable: bool,
    pub repeated: bool,
    pub null_count: Option<String>,
    pub null_rate: Option<f64>,
    pub min: Option<String>,
    pub max: Option<String>,
    pub min_max_truncated: bool,
    pub stats_complete: bool,
    pub distinct: Option<String>,
    pub compression: String,
    pub encodings: String,
    pub compressed_bytes: String,
    pub compressed_ratio: f64,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMeta {
    #[serde(skip)]
    pub modified: Option<std::time::SystemTime>,
    pub name: String,
    pub path: String,
    pub size_bytes: String,
    pub num_rows: String,
    pub row_groups: usize,
    pub created_by: Option<String>,
    pub version: i32,
    pub column_count: usize,
    pub columns: Vec<ColumnInfo>,
    pub metadata_bytes_read: u64,
}

pub fn failure(code: &str, detail: impl ToString) -> OpenError {
    OpenError {
        code: code.into(),
        detail: detail.to_string(),
    }
}

/// Exactly three bounded reads: header magic, footer trailer, serialized footer.
/// Does not instantiate a record/page reader or read any data page.
pub fn read_footer<R: Read + Seek>(
    reader: &mut R,
    size: u64,
) -> Result<(ParquetMetaData, u64), OpenError> {
    if size < 12 {
        return Err(failure(
            "parse_failed",
            "File is shorter than the Parquet header and footer",
        ));
    }
    let mut magic = [0; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| failure("unreadable", e))?;
    reader
        .seek(SeekFrom::End(-8))
        .map_err(|e| failure("unreadable", e))?;
    let mut tail = [0; 8];
    reader
        .read_exact(&mut tail)
        .map_err(|e| failure("unreadable", e))?;
    if &magic == b"PARE" || &tail[4..] == b"PARE" {
        return Err(failure("encrypted", "Encrypted Parquet footer"));
    }
    if &magic != b"PAR1" || &tail[4..] != b"PAR1" {
        return Err(failure("parse_failed", "Invalid Parquet magic"));
    }
    let len = u32::from_le_bytes(tail[..4].try_into().unwrap()) as u64;
    if len == 0 || len > size - 12 {
        return Err(failure("parse_failed", "Invalid footer length"));
    }
    if len > 128 * 1024 * 1024 {
        return Err(failure(
            "resource_limit",
            "Footer exceeds 128 MiB safety limit",
        ));
    }
    reader
        .seek(SeekFrom::Start(size - 8 - len))
        .map_err(|e| failure("unreadable", e))?;
    let mut bytes = vec![0; len as usize];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| failure("unreadable", e))?;
    if crate::footer::encrypted(&bytes).map_err(|e| failure("parse_failed", e))? {
        return Err(failure(
            "encrypted",
            "Plaintext footer with encrypted columns",
        ));
    }
    let meta =
        ParquetMetaDataReader::decode_metadata(&bytes).map_err(|e| failure("parse_failed", e))?;
    Ok((meta, len + 12))
}

pub fn summarize(
    meta: &ParquetMetaData,
    name: String,
    path: String,
    size: u64,
    read: u64,
) -> Result<FileMeta, OpenError> {
    let fm = meta.file_metadata();
    let schema = fm.schema_descr();
    let rows =
        u64::try_from(fm.num_rows()).map_err(|_| failure("parse_failed", "Negative row count"))?;
    let mut row_sum = 0u64;
    for rg in meta.row_groups() {
        let n = u64::try_from(rg.num_rows())
            .map_err(|_| failure("parse_failed", "Negative row group rows"))?;
        row_sum = row_sum
            .checked_add(n)
            .ok_or_else(|| failure("parse_failed", "Row count overflow"))?;
        if rg.num_columns() != schema.num_columns() {
            return Err(failure(
                "parse_failed",
                "Row group/schema column count mismatch",
            ));
        }
    }
    if row_sum != rows {
        return Err(failure(
            "parse_failed",
            "Row group totals do not match file rows",
        ));
    }
    let mut columns = Vec::with_capacity(schema.num_columns());
    for (index, col) in schema.columns().iter().enumerate() {
        let kind = Kind::of(col);
        let repeated = col.max_rep_level() > 0;
        let mut nulls = 0u64;
        let mut complete = true;
        let mut lower: Option<Scalar> = None;
        let mut upper: Option<Scalar> = None;
        let mut approximate = matches!(kind, Kind::String);
        let mut codecs = BTreeSet::new();
        let mut encodings = BTreeSet::new();
        let mut bytes = 0u64;
        let mut notes = BTreeSet::new();
        if repeated {
            notes.insert("repeated_null".to_string());
        }
        if matches!(kind, Kind::Unsupported(_)) {
            notes.insert("unsupported_type".to_string());
        }
        for rg in meta.row_groups() {
            let chunk = rg.column(index);
            codecs.insert(format!("{:?}", chunk.compression_codec()));
            for encoding in chunk.encodings() {
                encodings.insert(format!("{encoding:?}"));
            }
            let compressed = u64::try_from(chunk.compressed_size())
                .map_err(|_| failure("parse_failed", "Negative compressed size"))?;
            bytes = bytes
                .checked_add(compressed)
                .ok_or_else(|| failure("parse_failed", "Compressed size overflow"))?;
            if rg.num_rows() == 0 {
                continue;
            }
            let Some(stats) = chunk.statistics() else {
                complete = false;
                continue;
            };
            let Some(n) = stats.null_count_opt() else {
                complete = false;
                continue;
            };
            let values = u64::try_from(chunk.num_values())
                .map_err(|_| failure("parse_failed", "Negative value count"))?;
            if n > values || (!repeated && values != rg.num_rows() as u64) {
                complete = false;
                continue;
            }
            nulls = nulls
                .checked_add(n)
                .ok_or_else(|| failure("parse_failed", "Null count overflow"))?;
            if n == values {
                continue;
            } // All-null group legitimately has no bounds.
            if col.sort_order() == SortOrder::UNDEFINED
                || (stats.is_min_max_deprecated()
                    && (col.sort_order() == SortOrder::UNSIGNED
                        || matches!(
                            col.physical_type(),
                            parquet::basic::Type::BYTE_ARRAY
                                | parquet::basic::Type::FIXED_LEN_BYTE_ARRAY
                        )))
            {
                complete = false;
                notes.insert("unsafe_order".into());
                continue;
            }
            approximate |= !stats.min_is_exact() || !stats.max_is_exact();
            let (Some(lo), Some(hi)) = (
                format::bound(stats, &kind, true),
                format::bound(stats, &kind, false),
            ) else {
                complete = false;
                continue;
            };
            if !matches!(
                lo.compare(&hi),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            ) {
                complete = false;
                continue;
            }
            if lower
                .as_ref()
                .is_none_or(|v| lo.compare(v) == Some(std::cmp::Ordering::Less))
            {
                lower = Some(lo);
            }
            if upper
                .as_ref()
                .is_none_or(|v| hi.compare(v) == Some(std::cmp::Ordering::Greater))
            {
                upper = Some(hi);
            }
        }
        let min = lower
            .as_ref()
            .and_then(|v| format::display(v, &kind, approximate));
        let max = upper
            .as_ref()
            .and_then(|v| format::display(v, &kind, approximate));
        if (lower.is_some() && min.is_none()) || (upper.is_some() && max.is_none()) {
            complete = false;
            notes.insert("unrepresentable".into());
        }
        if !complete {
            notes.insert("incomplete".into());
        }
        if approximate && min.is_some() {
            notes.insert("approximate".into());
        }
        columns.push(ColumnInfo {
            index,
            path: col.path().string(),
            logical_type: kind.label(col.physical_type(), col.type_length()),
            physical_type: format!("{:?}", col.physical_type()),
            nullable: col.max_def_level() > 0,
            repeated,
            null_count: if complete && !repeated {
                Some(nulls.to_string())
            } else {
                None
            },
            null_rate: if complete && !repeated && rows > 0 {
                Some(nulls as f64 / rows as f64)
            } else {
                None
            },
            min: if complete { min } else { None },
            max: if complete { max } else { None },
            min_max_truncated: approximate,
            stats_complete: complete,
            distinct: None,
            compression: codecs.into_iter().collect::<Vec<_>>().join(", "),
            encodings: encodings.into_iter().collect::<Vec<_>>().join(", "),
            compressed_bytes: bytes.to_string(),
            compressed_ratio: if size > 0 {
                bytes as f64 / size as f64
            } else {
                0.0
            },
            notes: notes.into_iter().collect(),
        });
    }
    Ok(FileMeta {
        modified: None,
        name,
        path,
        size_bytes: size.to_string(),
        num_rows: rows.to_string(),
        row_groups: meta.num_row_groups(),
        created_by: fm.created_by().map(str::to_owned),
        version: fm.version(),
        column_count: columns.len(),
        columns,
        metadata_bytes_read: read,
    })
}
