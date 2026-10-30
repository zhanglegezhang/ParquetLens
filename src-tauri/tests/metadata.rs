use parquetlens_lib::{
    files::inspect_file,
    meta::{FileMeta, read_footer, summarize},
};
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/m1")
        .join(name)
}
fn load(name: &str) -> FileMeta {
    inspect_file(fixture(name).to_str().unwrap()).unwrap()
}

#[test]
fn independent_pyarrow_numeric_oracle() {
    let meta = load("types.parquet");
    assert_eq!(meta.num_rows, "6");
    assert_eq!(meta.row_groups, 3);
    assert_eq!(meta.column_count, 37);
    let expected: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("expected.json")).unwrap()).unwrap();
    for (name, values) in expected.as_object().unwrap() {
        let col = meta.columns.iter().find(|c| &c.path == name).unwrap();
        assert_eq!(col.min.as_deref(), values["min"].as_str(), "min {name}");
        assert_eq!(col.max.as_deref(), values["max"].as_str(), "max {name}");
        assert_eq!(col.null_count.as_deref(), Some("1"), "nulls {name}");
        assert!((col.null_rate.unwrap() - 1.0 / 6.0).abs() < 1e-12);
        assert!(col.stats_complete, "complete {name}");
    }
}

#[test]
fn dates_times_strings_binary_and_nested() {
    let m = load("types.parquet");
    let c = |name: &str| m.columns.iter().find(|c| c.path == name).unwrap();
    assert_eq!(c("date32").min.as_deref(), Some("1800-01-01"));
    assert_eq!(c("date64").max.as_deref(), Some("2024-02-29"));
    assert_eq!(
        c("ts_ns_naive").min.as_deref(),
        Some("1969-12-31 23:59:59.999999999")
    );
    assert_eq!(
        c("ts_us_UTC").min.as_deref(),
        Some("1969-12-31 23:59:59.999999")
    );
    assert!(c("ts_ns_naive").logical_type.contains("无时区"));
    assert!(c("ts_us_UTC").logical_type.contains("UTC"));
    assert_eq!(c("time64ns").max.as_deref(), Some("23:59:59.999999999"));
    assert_eq!(c("time32").max.as_deref(), Some("23:59:59.999"));
    assert_eq!(c("text").min.as_deref(), Some(""));
    assert_eq!(c("text").max.as_deref(), Some("🦀"));
    assert!(c("text").min_max_truncated);
    assert_eq!(
        c("uuid").min.as_deref(),
        Some("00000000-0000-0000-0000-000000000000")
    );
    assert!(
        c("fixed16")
            .min
            .as_ref()
            .unwrap()
            .starts_with("<binary 16 bytes>")
    );
    assert!(
        c("binary")
            .max
            .as_ref()
            .unwrap()
            .starts_with("<binary 20 bytes>")
    );
    assert!(c("dictionary").encodings.contains("DICTIONARY"));
    assert!(m.columns.iter().any(|c| c.path == "struct.a"));
    for col in m.columns.iter().filter(|c| c.repeated) {
        assert!(col.null_rate.is_none());
        assert!(col.null_count.is_none());
    }
    assert!(c("all_null").stats_complete);
    assert_eq!(c("all_null").null_count.as_deref(), Some("6"));
    assert_eq!(c("all_null").null_rate, Some(1.0));
    assert!(c("all_null").min.is_none());
    assert_eq!(c("null_type").logical_type, "Null");
    assert_eq!(c("bool").min.as_deref(), Some("false"));
    assert_eq!(c("bool").max.as_deref(), Some("true"));
    assert!(!c("f64").min.as_ref().unwrap().contains('e'));
}

#[test]
fn no_stats_never_means_zero() {
    for c in load("no_stats.parquet").columns {
        assert!(!c.stats_complete);
        assert!(c.null_count.is_none());
        assert!(c.min.is_none());
        assert!(c.max.is_none());
    }
}

#[test]
fn one_incomplete_group_invalidates_only_that_column() {
    let original = std::fs::read(fixture("types.parquet")).unwrap();
    let absent = std::fs::read(fixture("no_stats.parquet")).unwrap();
    let (m, _) = read_footer(&mut Cursor::new(&original), original.len() as u64).unwrap();
    let (missing, _) = read_footer(&mut Cursor::new(&absent), absent.len() as u64).unwrap();
    let mut groups = m.row_groups().to_vec();
    let mut cols = groups[1].columns().to_vec();
    cols[0] = missing.row_group(1).column(0).clone();
    groups[1] = groups[1]
        .clone()
        .into_builder()
        .set_column_metadata(cols)
        .build()
        .unwrap();
    let changed = m.into_builder().set_row_groups(groups).build();
    let result = summarize(
        &changed,
        "test".into(),
        "test".into(),
        original.len() as u64,
        0,
    )
    .unwrap();
    assert!(!result.columns[0].stats_complete);
    assert!(result.columns[0].min.is_none());
    assert!(result.columns[0].null_count.is_none());
    assert!(result.columns[1].stats_complete);
}

#[test]
fn empty_all_null_and_nan_semantics() {
    let m = load("empty.parquet");
    assert_eq!(m.num_rows, "0");
    for c in m.columns {
        assert!(c.null_rate.is_none());
        assert!(c.min.is_none());
    }
    let m = load("all_null.parquet");
    assert_eq!(m.columns[0].null_count.as_deref(), Some("2"));
    assert!(m.columns[0].stats_complete);
    let m = load("special_float.parquet");
    assert!(!m.columns[0].stats_complete);
    assert_eq!(m.columns[1].min.as_deref(), Some("-Inf"));
    assert_eq!(m.columns[1].max.as_deref(), Some("Inf"));
}

#[test]
fn compression_and_wide_schema() {
    for codec in ["snappy", "gzip", "zstd", "lz4", "brotli"] {
        let m = load(&format!("{codec}.parquet"));
        assert!(m.columns[0].compression.to_lowercase().contains(codec));
        assert_eq!(m.columns[0].min.as_deref(), Some("1"));
        assert_eq!(m.columns[0].max.as_deref(), Some("4"));
    }
    assert_eq!(load("wide.parquet").column_count, 601);
}

#[test]
fn encrypted_and_corrupt_files() {
    for file in ["encrypted_False.parquet", "encrypted_True.parquet"] {
        assert_eq!(
            inspect_file(fixture(file).to_str().unwrap())
                .unwrap_err()
                .code,
            "encrypted",
            "{file}"
        );
    }
    for file in ["corrupt.parquet", "zero.parquet"] {
        assert_eq!(
            inspect_file(fixture(file).to_str().unwrap())
                .unwrap_err()
                .code,
            "parse_failed"
        );
    }
    let mut bad = b"PAR1junk".to_vec();
    bad.extend_from_slice(&u32::MAX.to_le_bytes());
    bad.extend_from_slice(b"PAR1");
    assert!(read_footer(&mut Cursor::new(&bad), bad.len() as u64).is_err());
}

struct Audited {
    cursor: Cursor<Vec<u8>>,
    ranges: Vec<(u64, usize)>,
}
impl Read for Audited {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let at = self.cursor.position();
        let n = self.cursor.read(out)?;
        self.ranges.push((at, n));
        Ok(n)
    }
}
impl Seek for Audited {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.cursor.seek(pos)
    }
}
#[test]
fn reads_no_data_pages() {
    let bytes = std::fs::read(fixture("types.parquet")).unwrap();
    let size = bytes.len() as u64;
    let length =
        u32::from_le_bytes(bytes[bytes.len() - 8..bytes.len() - 4].try_into().unwrap()) as u64;
    let mut audit = Audited {
        cursor: Cursor::new(bytes),
        ranges: vec![],
    };
    let (_, read) = read_footer(&mut audit, size).unwrap();
    assert_eq!(read, length + 12);
    assert_eq!(audit.ranges.len(), 3);
    for (at, n) in audit.ranges {
        assert!(
            (at == 0 && n == 4) || at >= size - length - 8,
            "read body at {at}"
        );
    }
}
