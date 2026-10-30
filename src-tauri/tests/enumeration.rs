use arrow::array::{Float64Array, StringArray};
use calamine::{Data, Reader, Xlsx, open_workbook};
use parquetlens_lib::{
    enum_cache::Cache,
    enum_count::{Budget, Counter, TASK_BYTES},
    enum_export, enum_store,
    enums::{self, EnumResult, Options},
    files,
    meta::FileMeta,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn fixture(name: &str) -> FileMeta {
    files::inspect_file(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/m3")
            .join(name)
            .to_str()
            .unwrap(),
    )
    .unwrap()
}
fn options(columns: Vec<usize>) -> Options {
    Options {
        columns,
        cap: 50,
        mode: "exact".into(),
        k: 1,
        force: false,
    }
}
fn analyze(meta: &FileMeta, opt: &Options, cache: &Cache) -> enums::Outcome {
    enums::analyze(meta, opt, &AtomicBool::new(false), cache, |_| {}, |_| {}).unwrap()
}
fn counts(r: &EnumResult) -> BTreeMap<Option<String>, String> {
    r.values
        .iter()
        .map(|v| (v.value.clone(), v.count.clone()))
        .collect()
}
fn get<'a>(r: &'a enums::Outcome, name: &str) -> &'a EnumResult {
    r.results.iter().find(|r| r.column.name == name).unwrap()
}

#[test]
fn candidate_catalog_and_nested_ownership() {
    let c = enums::catalog(&fixture("enums.parquet")).unwrap();
    for name in [
        "union_cap",
        "text",
        "constant",
        "all_null",
        "small_decimal",
        "date",
    ] {
        assert!(
            c.iter().find(|c| c.name == name).unwrap().candidate,
            "{name}"
        );
    }
    for name in [
        "float",
        "binary",
        "nested",
        "map",
        "big_decimal",
        "timestamp",
        "time",
    ] {
        assert!(
            !c.iter().find(|c| c.name == name).unwrap().candidate,
            "{name}"
        );
    }
    let nested = c.iter().find(|c| c.name == "nested").unwrap();
    assert!(nested.nested);
    assert_eq!(nested.leaves.len(), 2);
}
#[test]
fn exact_counts_cap_null_and_full_binary() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let m = fixture("enums.parquet");
    let out = analyze(&m, &options((0..13).collect()), &cache);
    assert!(!out.cancelled);
    assert!(out.warnings.is_empty());
    let high = get(&out, "union_cap");
    assert_eq!(high.status, "high");
    assert!(high.values.is_empty());
    assert_eq!(high.distinct, None);
    let text = get(&out, "text");
    assert_eq!(text.distinct, Some(4));
    assert_eq!(text.scanned, "120");
    let c = counts(text);
    assert_eq!(c[&None], "20");
    assert_eq!(c[&Some("".into())], "20");
    assert_eq!(c[&Some("<NULL>".into())], "20");
    assert_eq!(c[&Some("a".into())], "40");
    let null = get(&out, "all_null");
    assert_eq!(null.distinct, Some(0));
    assert_eq!(counts(null)[&None], "120");
    let f = get(&out, "float");
    assert_eq!(f.distinct, Some(5));
    let c = counts(f);
    assert_eq!(c[&Some("NaN".into())], "40");
    assert_eq!(c[&Some("-0".into())], "20");
    assert_eq!(c[&Some("0".into())], "20");
    let b = get(&out, "binary");
    assert_eq!(b.distinct, Some(2));
    assert!(
        b.values
            .iter()
            .filter_map(|v| v.value.as_ref())
            .all(|s| s.len() > 65)
    );
    assert_eq!(get(&out, "nested").distinct, Some(2));
    assert_eq!(get(&out, "map").distinct, Some(2));
    for r in out.results.iter().filter(|r| r.status == "complete") {
        assert!(r.valid_cache());
        assert_eq!(
            r.values
                .iter()
                .map(|v| v.count.parse::<u64>().unwrap())
                .sum::<u64>(),
            120
        );
    }
}
#[test]
fn quick_scope_and_parameter_separated_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let m = fixture("enums.parquet");
    let mut opt = options(vec![0, 1]);
    opt.mode = "quick".into();
    let a = analyze(&m, &opt, &cache);
    assert!(!a.results[0].exact);
    assert_eq!(get(&a, "union_cap").distinct, Some(40));
    assert_eq!(get(&a, "text").scope_rows, "60");
    assert!(
        analyze(&m, &opt, &cache)
            .results
            .iter()
            .all(|r| r.source == "cache")
    );
    opt.k = 2;
    let b = analyze(&m, &opt, &cache);
    assert!(b.results.iter().all(|r| r.exact && r.source != "cache"));
    assert_eq!(get(&b, "union_cap").status, "high");
    opt.cap = 100;
    assert_eq!(
        get(&analyze(&m, &opt, &cache), "union_cap").distinct,
        Some(70)
    );
    opt.force = true;
    assert!(
        analyze(&m, &opt, &cache)
            .results
            .iter()
            .all(|r| r.source != "cache")
    );
}
#[test]
fn dictionaries_merge_values_not_ids_and_ignore_unused() {
    let temp = tempfile::tempdir().unwrap();
    let out = analyze(
        &fixture("dictionaries.parquet"),
        &options(vec![0]),
        &Cache::new(temp.path().into()),
    );
    let c = counts(&out.results[0]);
    assert_eq!(out.results[0].distinct, Some(2));
    assert_eq!(c[&Some("a".into())], "4");
    assert_eq!(c[&Some("b".into())], "2");
    assert_eq!(c[&None], "2");
}
#[test]
fn footer_shortcut_never_reads_corrupt_data_and_empty_has_no_null_bucket() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let a = analyze(
        &fixture("constant_damaged.parquet"),
        &options(vec![0]),
        &cache,
    );
    assert_eq!(a.results[0].source, "footer");
    assert_eq!(counts(&a.results[0])[&Some("42".into())], "100");
    let a = analyze(&fixture("empty.parquet"), &options(vec![0]), &cache);
    assert_eq!(a.results[0].distinct, Some(0));
    assert!(a.results[0].values.is_empty());
}
#[test]
fn resource_limit_is_not_high_and_not_cached() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let m = fixture("resource.parquet");
    for _ in 0..2 {
        let a = analyze(&m, &options(vec![0]), &cache);
        assert_eq!(a.results[0].status, "resource");
        assert_eq!(a.results[0].source, "scan");
        assert!(a.results[0].values.is_empty());
    }
}
#[test]
fn task_budget_and_counter_release_and_float_bits() {
    let budget = Arc::new(Budget::default());
    budget.0.store(TASK_BYTES, Ordering::Relaxed);
    let mut c = Counter::new(50, budget.clone());
    c.add(
        &StringArray::from(vec!["a"]),
        false,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(c.status, "resource");
    assert_eq!(budget.0.load(Ordering::Relaxed), TASK_BYTES);
    budget.0.store(0, Ordering::Relaxed);
    let mut c = Counter::new(50, budget.clone());
    c.add(
        &Float64Array::from(vec![f64::NAN, f64::from_bits(0x7ff8000000000001), 0., -0.]),
        false,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(c.values.len(), 3);
    assert!(budget.0.load(Ordering::Relaxed) > 0);
    drop(c);
    assert_eq!(budget.0.load(Ordering::Relaxed), 0);
}
#[test]
fn cache_corruption_fallback_clear_generation_and_unavailable_directory() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().join("cache"));
    let m = fixture("enums.parquet");
    let opt = options(vec![1]);
    analyze(&m, &opt, &cache);
    let p = std::fs::read_dir(&cache.dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&p, b"broken json").unwrap();
    let a = analyze(&m, &opt, &cache);
    assert_eq!(a.results[0].source, "scan");
    assert!(!a.warnings.is_empty());
    assert_eq!(analyze(&m, &opt, &cache).results[0].source, "cache");
    let epoch = cache.epoch();
    cache.clear().unwrap();
    assert!(!cache.write(&m.path, "x", BTreeMap::new(), epoch).unwrap());
    assert_eq!(cache.size().unwrap(), 0);
    let blocked = temp.path().join("file");
    std::fs::write(&blocked, b"x").unwrap();
    let a = analyze(&m, &opt, &Cache::new(blocked));
    assert_eq!(a.results[0].status, "complete");
    assert!(!a.warnings.is_empty());
}
#[test]
fn cache_lru_and_oversize_keep_unrelated_files() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::with_limit(temp.path().into(), 2500);
    std::fs::write(temp.path().join("keep.txt"), b"safe").unwrap();
    let m = fixture("enums.parquet");
    let seed = tempfile::tempdir().unwrap();
    let r = analyze(&m, &options(vec![1]), &Cache::new(seed.path().into()))
        .results
        .remove(0);
    let entries = BTreeMap::from([("1:50:exact:1".into(), r)]);
    assert!(cache.write("a", "f", entries.clone(), 0).unwrap());
    assert!(cache.write("b", "f", entries.clone(), 0).unwrap());
    cache.read("a", "f").unwrap();
    assert!(cache.write("c", "f", entries.clone(), 0).unwrap());
    assert!(cache.size().unwrap() <= 2500);
    assert!(!cache.read("a", "f").unwrap().is_empty());
    let tiny = Cache::with_limit(temp.path().join("tiny"), 100);
    assert!(!tiny.write("a", "f", entries, 0).unwrap());
    cache.clear().unwrap();
    assert_eq!(
        std::fs::read(temp.path().join("keep.txt")).unwrap(),
        b"safe"
    );
}
#[test]
fn cancellation_keeps_footer_completed_and_clearing_during_task_wins() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let cancel = AtomicBool::new(false);
    let m = fixture("enums.parquet");
    let out = enums::analyze(
        &m,
        &options(vec![2, 3]),
        &cancel,
        &cache,
        |r| {
            if r.column.root == 2 {
                cache.clear().unwrap();
                cancel.store(true, Ordering::Relaxed);
            }
        },
        |_| {},
    )
    .unwrap();
    assert!(out.cancelled);
    assert_eq!(get(&out, "constant").status, "complete");
    assert_eq!(get(&out, "all_null").status, "cancelled");
    assert_eq!(cache.size().unwrap(), 0);
}
#[test]
fn source_replacement_invalidates_session_and_cache() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("source.parquet");
    std::fs::copy(&fixture("enums.parquet").path, &path).unwrap();
    let m = files::inspect_file(path.to_str().unwrap()).unwrap();
    let cache = Cache::new(temp.path().join("cache"));
    analyze(&m, &options(vec![1]), &cache);
    std::fs::copy(&fixture("dictionaries.parquet").path, &path).unwrap();
    assert_eq!(
        enums::analyze(
            &m,
            &options(vec![1]),
            &AtomicBool::new(false),
            &cache,
            |_| {},
            |_| {}
        )
        .err()
        .unwrap()
        .code,
        "file_changed"
    );
    let m = files::inspect_file(path.to_str().unwrap()).unwrap();
    assert_eq!(
        analyze(&m, &options(vec![0]), &cache).results[0].source,
        "scan"
    );
}
#[test]
fn numeric_sort_search_pages_and_store_owner() {
    let temp = tempfile::tempdir().unwrap();
    let mut r = analyze(
        &fixture("enums.parquet"),
        &options(vec![8]),
        &Cache::new(temp.path().into()),
    )
    .results
    .remove(0);
    let p = enum_store::page(&r, 0, 50, "", "value", false).unwrap();
    assert_eq!(p.rows[0].value.as_deref(), Some("-2.10"));
    assert_eq!(
        enum_store::page(&r, 0, 50, "12.30", "value", false)
            .unwrap()
            .total,
        1
    );
    assert!(enum_store::page(&r, 0, 501, "", "value", false).is_err());
    let mut nums = [
        "10000000000000000001",
        "9007199254740993",
        "-12.2",
        "-2.1",
        "0",
        "-0",
        "0.01",
        "0.1",
        "-Inf",
        "Inf",
        "NaN",
    ];
    nums.sort_by(|a, b| enum_store::decimal_cmp(a, b));
    assert_eq!(
        nums,
        [
            "-Inf",
            "-12.2",
            "-2.1",
            "-0",
            "0",
            "0.01",
            "0.1",
            "9007199254740993",
            "10000000000000000001",
            "Inf",
            "NaN"
        ]
    );
    let store = enum_store::Store::default();
    r = store.put("file", "request", &r).unwrap();
    assert!(r.values.is_empty());
    assert!(store.get("wrong", &r.id).is_err());
    assert!(store.get("file", &r.id).is_ok());
    store.close("file");
    assert!(store.get("file", &r.id).is_err());
}
#[test]
fn xlsx_roundtrip_null_text_percent_sample_high_and_cancel_atomicity() {
    let temp = tempfile::tempdir().unwrap();
    let m = fixture("enums.parquet");
    let mut opt = options(vec![0, 1]);
    opt.mode = "quick".into();
    let out = analyze(&m, &opt, &Cache::new(temp.path().join("cache")));
    let p = temp.path().join("enums.xlsx");
    let r = get(&out, "text");
    let report = enum_export::export(&m, r, &p, &AtomicBool::new(false)).unwrap();
    assert_eq!(report.rows, 5);
    let mut book: Xlsx<_> = open_workbook(&p).unwrap();
    let sheet = book.worksheet_range("枚举值").unwrap();
    let mut total = 0.;
    let mut nulls = 0;
    for row in sheet.rows().skip(1) {
        if row[2] == Data::Bool(true) {
            nulls += 1;
            assert_eq!(row[1], Data::String("<NULL>".into()));
        }
        if let Data::Float(n) = row[4] {
            total += n;
        }
        assert!(row[5].to_string().contains("采样"));
    }
    assert_eq!(nulls, 1);
    assert!((total - 1.).abs() < 1e-12);
    let before = std::fs::read(&p).unwrap();
    assert!(enum_export::export(&m, r, &p, &AtomicBool::new(true)).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), before);
    drop(book);
    opt.mode = "exact".into();
    let high = analyze(&m, &opt, &Cache::new(temp.path().join("cache")));
    enum_export::export(&m, get(&high, "union_cap"), &p, &AtomicBool::new(false)).unwrap();
    let mut book: Xlsx<_> = open_workbook(&p).unwrap();
    let sheet = book.worksheet_range("枚举值").unwrap();
    assert!(sheet.get((1, 5)).unwrap().to_string().contains("> 50"));
    assert!(matches!(sheet.get((1, 3)), Some(Data::Empty)));
}

#[test]
fn summaries_preserve_scope_and_status_without_changing_stored_values() {
    let temp = tempfile::tempdir().unwrap();
    let cache = Cache::new(temp.path().into());
    let meta = fixture("enums.parquet");
    for mode in ["exact", "quick"] {
        let mut opt = options((0..13).collect());
        opt.mode = mode.into();
        let out = analyze(&meta, &opt, &cache);
        for result in out.results {
            let before = serde_json::to_value(&result).unwrap();
            let mut expected = before.clone();
            expected["values"] = serde_json::json!([]);
            assert_eq!(serde_json::to_value(result.summary()).unwrap(), expected);
            assert_eq!(serde_json::to_value(&result).unwrap(), before);
        }
    }
}

#[test]
fn stopped_counter_releases_its_budget_without_affecting_other_columns() {
    let budget = Arc::new(Budget::default());
    let cancel = AtomicBool::new(false);
    let mut survivor = Counter::new(50, budget.clone());
    survivor
        .add(&StringArray::from(vec!["kept"]), false, &cancel)
        .unwrap();
    let retained = budget.0.load(Ordering::Relaxed);
    let mut high = Counter::new(50, budget.clone());
    let strings: Vec<String> = (0..51).map(|i| format!("value-{i}")).collect();
    high.add(&StringArray::from(strings), false, &cancel)
        .unwrap();
    assert_eq!(high.status, "high");
    assert!(high.values.is_empty());
    assert_eq!(budget.0.load(Ordering::Relaxed), retained);
    drop(high);
    assert_eq!(budget.0.load(Ordering::Relaxed), retained);
    survivor
        .add(&StringArray::from(vec!["kept"]), false, &cancel)
        .unwrap();
    assert_eq!(survivor.status, "running");
    drop(survivor);
    assert_eq!(budget.0.load(Ordering::Relaxed), 0);
}
