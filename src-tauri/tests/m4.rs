use calamine::{Data, Reader, Xlsx, open_workbook};
use parquetlens_lib::{
    dictionary::{self, Context},
    display_time,
    enum_cache::Cache,
    enums, export_csv, files,
    meta::FileMeta,
    sample,
    settings::{Preferences, Settings},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
fn fixture(name: &str) -> FileMeta {
    files::inspect_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures")
            .join(name)
            .to_str()
            .unwrap(),
    )
    .unwrap()
}
fn req(m: &FileMeta) -> sample::Request {
    sample::Request {
        mode: "head".into(),
        n: 1000,
        columns: sample::fields(m).unwrap().iter().map(|f| f.index).collect(),
    }
}
fn dic(m: &FileMeta) -> dictionary::Request {
    dictionary::Request {
        include_sample: true,
        sample: req(m),
        cap: 50,
        existing_only: false,
        manual_columns: vec![],
    }
}
#[test]
fn csv_roundtrip_all_types_and_protection() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m1/types.parquet");
    let path = temp.path().join("sample.csv");
    let opts = req(&m);
    let report =
        export_csv::sample_export(&m, &opts, &path, true, &AtomicBool::new(false), |_| {}).unwrap();
    assert_eq!(report.rows, 6);
    assert!(report.protected >= 1);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\xef\xbb\xbf"));
    let mut csv = csv::Reader::from_reader(&bytes[3..]);
    let headers = csv.headers().unwrap().clone();
    let rows = csv
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    let id = headers.iter().position(|s| s == "id64").unwrap();
    let text = headers.iter().position(|s| s == "text").unwrap();
    assert_eq!(&rows[0][id], "9007199254740993");
    assert_eq!(&rows[3][text], "'=cmd");
    assert_eq!(&rows[4][text], "hello\tworld\nline");
    assert_eq!(&rows[1][text], "中文🦀");
}
#[test]
fn csv_preserves_long_controls_and_quotes_without_xlsx_truncation() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m2/edge.parquet");
    let path = temp.path().join("sample.csv");
    let r = export_csv::sample_export(&m, &req(&m), &path, false, &AtomicBool::new(false), |_| {})
        .unwrap();
    assert_eq!(r.truncated, 0);
    assert_eq!(r.protected, 0);
    let bytes = std::fs::read(&path).unwrap();
    let rows = csv::Reader::from_reader(&bytes[3..])
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(&rows[0][0], "中😀".repeat(20000));
    assert_eq!(&rows[1][0], "=cmd");
    assert_eq!(&rows[5][0], "\0a\x01b\t\nc");
    let mut bytes = vec![];
    assert_eq!(
        export_csv::row(
            &mut bytes,
            &[
                (None, true),
                (Some("".into()), true),
                (Some("a,\"b\"\r\nc".into()), true),
                (Some(" \t=cmd".into()), true),
                (Some("-1".into()), false)
            ],
            true
        )
        .unwrap(),
        1
    );
    assert!(bytes.starts_with(b",\"\","));
    let row = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(bytes.as_slice())
        .records()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(&row[2], "a,\"b\"\r\nc");
    assert_eq!(&row[3], "' \t=cmd");
    assert_eq!(&row[4], "-1");
}
#[test]
fn csv_cancel_and_failure_preserve_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m2/uneven.parquet");
    let path = temp.path().join("keep.csv");
    std::fs::write(&path, b"old").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        export_csv::sample_export(&m, &req(&m), &path, true, &cancel, |_| cancel
            .store(true, Ordering::Relaxed))
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    assert!(
        export_csv::sample_export(
            &m,
            &req(&m),
            &temp.path().join("source.parquet"),
            true,
            &AtomicBool::new(false),
            |_| {}
        )
        .is_err()
    );
}
#[test]
fn dictionary_combines_all_columns_enums_and_typed_sample() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m3/enums.parquet");
    let path = temp.path().join("dictionary.xlsx");
    let cache = Cache::new(temp.path().join("cache"));
    let r = dictionary::export(
        &m,
        &dic(&m),
        &path,
        Context {
            local: false,
            cache: &cache,
            existing: vec![],
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(r.rows, 120);
    let mut b: Xlsx<_> = open_workbook(path).unwrap();
    assert_eq!(b.sheet_names(), ["列清单", "枚举值", "样例"]);
    let columns = b.worksheet_range("列清单").unwrap();
    assert_eq!(columns.height(), m.column_count + 1);
    assert_eq!(columns.get((1, 9)), Some(&Data::String(">50".into())));
    let enums = b.worksheet_range("枚举值").unwrap();
    assert!(
        enums
            .rows()
            .any(|r| r[0] == "union_cap" && r[5].to_string().contains("基数 > 50"))
    );
    assert!(
        enums
            .rows()
            .any(|r| r[0] == "float" && r[5].to_string().contains("未分析"))
    );
    let sample = b.worksheet_range("样例").unwrap();
    assert_eq!(sample.height(), 121);
    assert_eq!(sample.get((1, 2)), Some(&Data::Float(7.)));
}
#[test]
fn dictionary_existing_only_marks_sample_missing_and_manual_nesting() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m3/enums.parquet");
    let cache = Cache::new(temp.path().join("cache"));
    let out = enums::analyze(
        &m,
        &enums::Options {
            columns: vec![1, 6],
            cap: 50,
            mode: "quick".into(),
            k: 1,
            force: false,
        },
        &AtomicBool::new(false),
        &cache,
        |_| {},
        |_| {},
    )
    .unwrap();
    let mut request = dic(&m);
    request.include_sample = false;
    request.existing_only = true;
    let path = temp.path().join("d.xlsx");
    dictionary::export(
        &m,
        &request,
        &path,
        Context {
            local: false,
            cache: &cache,
            existing: out.results.into_iter().map(Arc::new).collect(),
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    let mut b: Xlsx<_> = open_workbook(path).unwrap();
    assert_eq!(b.sheet_names(), ["列清单", "枚举值"]);
    let c = b.worksheet_range("列清单").unwrap();
    assert_eq!(c.get((2, 9)), Some(&Data::String("样本 4".into())));
    assert!(c.get((7, 9)).unwrap().to_string().contains("未分析"));
    let e = b.worksheet_range("枚举值").unwrap();
    assert!(
        e.rows()
            .any(|r| r[0] == "text" && r[5].to_string().contains("采样"))
    );
    assert!(
        e.rows()
            .any(|r| r[0] == "nested" && r[1].to_string().contains("\"x\""))
    );
}
#[test]
fn dictionary_cancel_and_corrupt_sample_never_replace_target() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m3/enums.parquet");
    let cache = Cache::new(temp.path().join("cache"));
    let path = temp.path().join("keep.xlsx");
    std::fs::write(&path, b"old").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        dictionary::export(
            &m,
            &dic(&m),
            &path,
            Context {
                local: false,
                cache: &cache,
                existing: vec![]
            },
            &cancel,
            |_, phase| {
                if phase == "columns" {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let m = fixture("m2/corrupt_column.parquet");
    let mut request = dic(&m);
    request.existing_only = true;
    assert!(
        dictionary::export(
            &m,
            &request,
            &path,
            Context {
                local: false,
                cache: &cache,
                existing: vec![]
            },
            &AtomicBool::new(false),
            |_, _| {}
        )
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"old");
}
#[test]
fn settings_defaults_persistence_validation_recent_limit_and_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("settings.json");
    let settings = Settings::new(path.clone());
    assert_eq!(settings.snapshot().preferences.cap, 200);
    let p = Preferences {
        cap: 500,
        sample_n: 77,
        theme: "dark".into(),
        time_mode: "local".into(),
        ..Default::default()
    };
    assert!(settings.save(p.clone()).unwrap().warning.is_none());
    for n in 0..12 {
        settings.recent(Some(format!("file{n}"))).unwrap();
    }
    settings.recent(Some("file5".into())).unwrap();
    let fresh = Settings::new(path.clone()).snapshot();
    assert_eq!(fresh.preferences.cap, 500);
    assert_eq!(fresh.preferences.recent.len(), 10);
    assert_eq!(fresh.preferences.recent[0], "file5");
    let mut bad = p;
    bad.cap = 49;
    assert!(settings.save(bad).is_err());
    assert_eq!(settings.snapshot().preferences.cap, 500);
    settings.recent(None).unwrap();
    assert!(settings.snapshot().preferences.recent.is_empty());
    std::fs::write(&path, b"bad json").unwrap();
    let fallback = Settings::new(path.clone()).snapshot();
    assert!(fallback.warning.is_some());
    assert_eq!(fallback.preferences.cap, 200);
    assert_eq!(std::fs::read(path).unwrap(), b"bad json");
}
#[test]
fn settings_unwritable_keeps_session_and_future_version_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("file");
    std::fs::write(&parent, b"block").unwrap();
    let settings = Settings::new(parent.join("settings.json"));
    let p = Preferences {
        cap: 300,
        ..Default::default()
    };
    let saved = settings.save(p).unwrap();
    assert!(saved.warning.is_some());
    assert_eq!(settings.snapshot().preferences.cap, 300);
    let path = temp.path().join("future.json");
    std::fs::write(&path, b"{\"version\":999}").unwrap();
    assert!(Settings::new(path).snapshot().warning.is_some());
}
#[test]
fn time_scope_precise_and_canonical_cache_unaffected() {
    use arrow::datatypes::{DataType, TimeUnit};
    let raw = "2026-01-02 03:04:05.123456789 UTC";
    let timestamp = DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()));
    assert_eq!(display_time::enum_text(raw, &timestamp), raw);
    {
        let _scope = display_time::Scope::new(true);
        let local = display_time::enum_text(raw, &timestamp);
        assert!(local.contains(".123456789 "));
        assert!(!local.ends_with("UTC"));
        assert_eq!(display_time::enum_text("1.00", &DataType::Utf8), "1.00");
        assert_eq!(
            display_time::enum_text(raw, &DataType::Timestamp(TimeUnit::Nanosecond, None)),
            raw
        );
    }
    assert!(!display_time::enabled());
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m3/enums.parquet");
    let cache = Cache::new(temp.path().join("cache"));
    let mut request = dic(&m);
    request.manual_columns = vec![11];
    request.include_sample = false;
    dictionary::export(
        &m,
        &request,
        &temp.path().join("local.xlsx"),
        Context {
            local: true,
            cache: &cache,
            existing: vec![],
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    let r = enums::analyze(
        &m,
        &enums::Options {
            columns: vec![11],
            cap: 50,
            mode: "exact".into(),
            k: 3,
            force: false,
        },
        &AtomicBool::new(false),
        &cache,
        |_| {},
        |_| {},
    )
    .unwrap();
    assert_eq!(r.results[0].source, "cache");
    assert!(
        r.results[0].values[0]
            .value
            .as_ref()
            .unwrap()
            .ends_with("无时区")
    );
    let m = fixture("m1/types.parquet");
    let root = enums::catalog(&m)
        .unwrap()
        .iter()
        .find(|c| c.name == "ts_ns_UTC")
        .unwrap()
        .root;
    let mut request = dic(&m);
    request.include_sample = false;
    request.manual_columns = vec![root];
    dictionary::export(
        &m,
        &request,
        &temp.path().join("utc-in-local-display.xlsx"),
        Context {
            local: true,
            cache: &cache,
            existing: vec![],
        },
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    let result = enums::analyze(
        &m,
        &enums::Options {
            columns: vec![root],
            cap: 50,
            mode: "exact".into(),
            k: 3,
            force: false,
        },
        &AtomicBool::new(false),
        &cache,
        |_| {},
        |_| {},
    )
    .unwrap();
    assert_eq!(result.results[0].source, "cache");
    assert!(
        result.results[0]
            .values
            .iter()
            .filter_map(|v| v.value.as_ref())
            .all(|v| v.ends_with(" UTC"))
    );
}
#[test]
fn enum_csv_counts_null_sample_and_high() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("m3/enums.parquet");
    let cache = Cache::new(temp.path().join("cache"));
    let opts = enums::Options {
        columns: vec![1],
        cap: 50,
        mode: "quick".into(),
        k: 1,
        force: false,
    };
    let result =
        enums::analyze(&m, &opts, &AtomicBool::new(false), &cache, |_| {}, |_| {}).unwrap();
    let path = temp.path().join("enum.csv");
    let report =
        export_csv::enum_export(&m, &result.results[0], &path, true, &AtomicBool::new(false))
            .unwrap();
    assert_eq!(report.rows, 5);
    let bytes = std::fs::read(&path).unwrap();
    let rows = csv::Reader::from_reader(&bytes[3..])
        .records()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|r| r[3].parse::<u64>().unwrap())
            .sum::<u64>(),
        60
    );
    assert_eq!(rows.iter().filter(|r| &r[2] == "true").count(), 1);
    assert!(rows.iter().all(|r| r[5].contains("采样")));
    let opts = enums::Options {
        columns: vec![0],
        mode: "exact".into(),
        ..opts
    };
    let high = enums::analyze(&m, &opts, &AtomicBool::new(false), &cache, |_| {}, |_| {}).unwrap();
    assert_eq!(
        export_csv::enum_export(&m, &high.results[0], &path, true, &AtomicBool::new(false))
            .unwrap()
            .rows,
        1
    );
}
