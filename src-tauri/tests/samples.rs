use calamine::{Data, DataType, Reader, Xlsx, open_workbook};
use parquetlens_lib::{
    export_xlsx, files,
    meta::FileMeta,
    sample::{self, Request},
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

fn fixture(name: &str) -> FileMeta {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(name);
    files::inspect_file(path.to_str().unwrap()).unwrap()
}
fn request(meta: &FileMeta, mode: &str, n: usize) -> Request {
    Request {
        mode: mode.into(),
        n,
        columns: sample::fields(meta)
            .unwrap()
            .iter()
            .map(|f| f.index)
            .collect(),
    }
}
fn read(path: &Path) -> calamine::Range<Data> {
    let mut book: Xlsx<_> = open_workbook(path).unwrap();
    book.worksheet_range("样例").unwrap()
}
#[test]
fn plan_quota_topup_and_head_are_deterministic() {
    assert_eq!(
        sample::plan(&[2, 1, 600, 600], "uniform", 10).unwrap(),
        [2, 1, 4, 3]
    );
    assert_eq!(
        sample::plan(&[2, 1, 600, 600], "head", 10).unwrap(),
        [2, 1, 7, 0]
    );
    assert_eq!(sample::plan(&[0, 0, 4], "uniform", 8).unwrap(), [0, 0, 4]);
    assert_eq!(
        sample::plan(&[3; 5], "uniform", 2).unwrap(),
        [1, 1, 0, 0, 0]
    );
    for n in [0, 1_000_001] {
        assert!(sample::plan(&[10], "head", n).is_err());
    }
    assert!(sample::plan(&[10], "random", 1).is_err());
}
#[test]
fn row_plan_properties_for_many_group_shapes() {
    for seed in 0..80 {
        let lens: Vec<_> = (0..7).map(|i| (seed * 13 + i * 7) % 31).collect();
        for mode in ["head", "uniform"] {
            for n in [1, 3, 17, 70, 1000] {
                let p = sample::plan(&lens, mode, n).unwrap();
                assert_eq!(p.iter().sum::<usize>(), n.min(lens.iter().sum()));
                assert!(p.iter().zip(&lens).all(|(a, b)| a <= b));
            }
        }
    }
}
#[test]
fn projection_paging_and_export_share_identical_selection() {
    let meta = fixture("m2/uneven.parquet");
    let cancel = AtomicBool::new(false);
    let mut req = request(&meta, "uniform", 10);
    req.columns = vec![2, 0, 0];
    let preview = sample::preview(&meta, &req, 0, 500, &cancel).unwrap();
    let ids: Vec<_> = preview
        .rows
        .iter()
        .map(|r| r[0].as_deref().unwrap().parse::<i64>().unwrap())
        .collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6, 603, 604, 605]);
    assert_eq!(preview.fields.len(), 2);
    assert_eq!(preview.rows[0][1].as_deref(), Some("18446744073709551615"));
    let page = sample::preview(&meta, &req, 7, 3, &cancel).unwrap();
    assert_eq!(page.rows, preview.rows[7..]);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("测试 样例.xlsx");
    export_xlsx::export(&meta, &req, &path, &cancel, |_, _| {}).unwrap();
    let range = read(&path);
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(
            range.get_value(((i + 1) as u32, 0)).unwrap().get_float(),
            Some(*id as f64)
        );
    }
    assert_eq!(
        range.get_value((1, 1)).unwrap().get_string(),
        Some("18446744073709551615")
    );
}
#[test]
fn paging_above_500_and_invalid_inputs() {
    let meta = fixture("m2/uneven.parquet");
    let cancel = AtomicBool::new(false);
    let mut req = request(&meta, "head", 1000);
    let p = sample::preview(&meta, &req, 500, 500, &cancel).unwrap();
    assert_eq!(p.rows.len(), 500);
    assert_eq!(p.rows[0][0].as_deref(), Some("500"));
    assert_eq!(p.rows[499][0].as_deref(), Some("999"));
    assert!(sample::preview(&meta, &req, 0, 501, &cancel).is_err());
    assert!(
        sample::preview(&meta, &req, 1000, 500, &cancel)
            .unwrap()
            .rows
            .is_empty()
    );
    req.columns.clear();
    assert!(sample::preview(&meta, &req, 0, 500, &cancel).is_err());
    req.columns = vec![999];
    assert!(sample::preview(&meta, &req, 0, 500, &cancel).is_err());
}
#[test]
fn all_types_preview_and_xlsx_readback() {
    let meta = fixture("m1/types.parquet");
    let req = request(&meta, "head", 1000);
    let cancel = AtomicBool::new(false);
    let preview = sample::preview(&meta, &req, 0, 500, &cancel).unwrap();
    assert_eq!(preview.rows.len(), 6);
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("types.xlsx");
    let report = export_xlsx::export(&meta, &req, &path, &cancel, |_, _| {}).unwrap();
    assert_eq!(report.rows, 6);
    let range = read(&path);
    for (c, field) in preview.fields.iter().enumerate() {
        for (r, row) in preview.rows.iter().enumerate() {
            let data = range
                .get_value(((r + 1) as u32, c as u32))
                .unwrap_or(&Data::Empty);
            match &row[c] {
                None => assert_eq!(data, &Data::Empty, "{} row {r}", field.name),
                Some(text) => match data {
                    Data::String(s) => assert_eq!(s, text, "{} row {r}", field.name),
                    Data::Float(n) => {
                        assert_eq!(*n, text.parse::<f64>().unwrap(), "{} row {r}", field.name)
                    }
                    Data::Bool(b) => assert_eq!(b.to_string(), *text),
                    Data::DateTime(_) => assert!(data.as_datetime().is_some()),
                    Data::Empty if text.is_empty() => {}
                    other => panic!("{} row {r}: unexpected {other:?}", field.name),
                },
            }
        }
    }
    let mut book: Xlsx<_> = open_workbook(path).unwrap();
    assert!(
        book.worksheet_formula("样例")
            .unwrap()
            .cells()
            .all(|(_, _, v)| v.is_empty())
    );
}
#[test]
fn edge_precision_json_controls_and_excel_epoch() {
    let meta = fixture("m2/edge.parquet");
    let req = request(&meta, "head", 6);
    let cancel = AtomicBool::new(false);
    let preview = sample::preview(&meta, &req, 0, 500, &cancel).unwrap();
    assert_eq!(
        preview.rows[0][3].as_deref(),
        Some("[\"9007199254740993\",null]")
    );
    assert_eq!(
        preview.rows[0][4].as_deref(),
        Some("[{\"key\":1,\"value\":\"a\"},{\"key\":1,\"value\":\"b\"}]")
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("edge.xlsx");
    let report = export_xlsx::export(&meta, &req, &path, &cancel, |_, _| {}).unwrap();
    assert_eq!(report.truncated, 1);
    assert_eq!(report.cleaned, 1);
    let range = read(&path);
    let long = range.get_value((1, 0)).unwrap().get_string().unwrap();
    assert!(long.ends_with("…(truncated)"));
    assert!(long.encode_utf16().count() <= 32767);
    assert_eq!(
        range.get_value((6, 0)).unwrap().get_string(),
        Some("ab\t\nc")
    );
    assert_eq!(
        range.get_value((1, 1)).unwrap().get_string(),
        Some("1.2345678901234567")
    );
    assert_eq!(range.get_value((3, 1)).unwrap().get_string(), Some("-0"));
    assert_eq!(range.get_value((4, 1)).unwrap().get_string(), Some("NaN"));
    assert_eq!(range.get_value((1, 2)).unwrap().get_float(), Some(1.2));
    for r in 1..=6 {
        let expected = preview.rows[r - 1][5].as_ref().unwrap();
        let d = range.get_value((r as u32, 5)).unwrap();
        if let Some(dt) = d.as_datetime() {
            assert_eq!(dt.format("%Y-%m-%d").to_string(), *expected);
        } else {
            assert_eq!(d.get_string(), Some(expected.as_str()));
        }
    }
}
#[test]
fn empty_all_null_and_no_statistics_files() {
    for name in ["empty", "all_null", "no_stats"] {
        let meta = fixture(&format!("m1/{name}.parquet"));
        let req = request(&meta, "uniform", 1000);
        let cancel = AtomicBool::new(false);
        let p = sample::preview(&meta, &req, 0, 500, &cancel).unwrap();
        assert_eq!(p.total, meta.num_rows.parse::<usize>().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("sample.xlsx");
        let r = export_xlsx::export(&meta, &req, &out, &cancel, |_, _| {}).unwrap();
        assert_eq!(r.rows, p.total);
        assert!(out.is_file());
    }
}
#[test]
fn cancellation_preserves_target_and_removes_temporary_files() {
    let meta = fixture("m2/uneven.parquet");
    let req = request(&meta, "head", 1203);
    let cancel = AtomicBool::new(false);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("sample.xlsx");
    std::fs::write(&out, b"existing").unwrap();
    let error = export_xlsx::export(&meta, &req, &out, &cancel, |_, _| {
        cancel.store(true, Ordering::Relaxed)
    })
    .unwrap_err();
    assert_eq!(error.code, "cancelled");
    assert_eq!(std::fs::read(&out).unwrap(), b"existing");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(
        sample::preview(&meta, &req, 0, 500, &cancel)
            .unwrap_err()
            .code,
        "cancelled"
    );
}
#[test]
fn source_changes_require_reopen() {
    let meta = fixture("m2/uneven.parquet");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.parquet");
    std::fs::copy(&meta.path, &path).unwrap();
    let meta = files::inspect_file(path.to_str().unwrap()).unwrap();
    std::fs::write(&path, b"changed").unwrap();
    assert_eq!(sample::fields(&meta).unwrap_err().code, "file_changed");
}
#[test]
fn text_and_numeric_boundaries() {
    assert!(export_xlsx::safe_number("999999999999999").is_some());
    assert!(export_xlsx::safe_number("1000000000000000").is_none());
    assert!(export_xlsx::safe_number("1.2345678901234567").is_none());
    assert!(export_xlsx::safe_number("0.00000000000000000001").is_some());
    let s = "😀".repeat(32767);
    let (out, truncated, cleaned) = export_xlsx::clean(&s);
    assert!(truncated && !cleaned);
    assert!(out.encode_utf16().count() <= 32767);
}
#[test]
fn projection_and_early_stop_avoid_damaged_unrequested_data() {
    let cancel = AtomicBool::new(false);
    let meta = fixture("m2/corrupt_column.parquet");
    let mut req = request(&meta, "head", 2);
    req.columns = vec![0];
    assert_eq!(
        sample::preview(&meta, &req, 0, 500, &cancel)
            .unwrap()
            .rows
            .len(),
        2
    );
    req.columns = vec![1];
    assert!(sample::preview(&meta, &req, 0, 500, &cancel).is_err());
    let meta = fixture("m2/corrupt_tail.parquet");
    req.columns = vec![0];
    req.n = 3;
    assert_eq!(
        sample::preview(&meta, &req, 0, 500, &cancel)
            .unwrap()
            .rows
            .len(),
        3
    );
    req.n = 1000;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("result.xlsx");
    std::fs::write(&path, b"previous").unwrap();
    assert!(export_xlsx::export(&meta, &req, &path, &cancel, |_, _| {}).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
#[cfg(windows)]
#[test]
fn locked_destination_fails_without_destroying_existing_data() {
    use std::os::windows::fs::OpenOptionsExt;
    let meta = fixture("m2/uneven.parquet");
    let req = request(&meta, "head", 3);
    let cancel = AtomicBool::new(false);
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("locked.xlsx");
    std::fs::write(&out, b"original").unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&out)
        .unwrap();
    assert_eq!(
        export_xlsx::export(&meta, &req, &out, &cancel, |_, _| {})
            .unwrap_err()
            .code,
        "export_failed"
    );
    drop(lock);
    assert_eq!(std::fs::read(&out).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    export_xlsx::export(&meta, &req, &out, &cancel, |_, _| {}).unwrap();
    assert_eq!(read(&out).height(), 4);
}

#[test]
fn all_enabled_codecs_decode_data_pages_after_size_optimization() {
    for codec in ["snappy", "gzip", "zstd", "lz4", "brotli"] {
        let meta = fixture(&format!("m1/{codec}.parquet"));
        let rows = sample::preview(
            &meta,
            &request(&meta, "head", 4),
            0,
            4,
            &AtomicBool::new(false),
        )
        .unwrap()
        .rows;
        assert_eq!(
            rows,
            vec![
                vec![Some("1".into())],
                vec![Some("2".into())],
                vec![None],
                vec![Some("4".into())]
            ],
            "{codec}"
        );
    }
}
