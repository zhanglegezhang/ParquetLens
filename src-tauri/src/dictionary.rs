use crate::{
    display_time,
    enum_cache::Cache,
    enums::{self, EnumResult},
    export_xlsx::{self, Report},
    meta::{FileMeta, failure},
    sample::{self, Result},
};
use rust_xlsxwriter::{Format, Workbook, Worksheet};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
#[derive(Clone, Deserialize)]
pub struct Request {
    pub include_sample: bool,
    pub sample: sample::Request,
    pub cap: usize,
    pub existing_only: bool,
    pub manual_columns: Vec<usize>,
}
pub fn scope(r: &EnumResult) -> String {
    let range = if r.exact {
        format!("全量 {} 行", r.scope_rows)
    } else {
        format!(
            "采样结果，非全量；前 {} / {} 个 row group，{} / {} 行",
            r.scope_groups, r.file_groups, r.scope_rows, r.file_rows
        )
    };
    match r.status.as_str() {
        "complete" => range,
        "high" => format!("基数 > {}；{range}；未提供部分频数", r.cap),
        "resource" => format!("资源限制；{range}；无完整频数"),
        "cancelled" => format!("已取消；{range}；无完整频数"),
        _ => format!("未分析；{range}"),
    }
}
fn err(e: impl ToString) -> crate::files::OpenError {
    failure("export_failed", e)
}
fn note(key: &str) -> &str {
    match key {
        "repeated_null" => "重复嵌套字段的 null 数口径不同于文件行数，未展示 null 数与比例。",
        "unsupported_type" => "当前不解释此类型的统计边界，保留类型信息。",
        "unsafe_order" => "旧版或未定义的统计排序规则无法安全合并，未显示全局统计。",
        "unrepresentable" => "部分统计值无法精确解释，未显示全局统计。",
        "incomplete" => "至少一个非空 row group 缺少有效统计，未扫描数据补全。",
        "approximate" => "边界可能经过截断；字符串统计保守标注为近似。",
        _ => key,
    }
}
fn text(s: &mut Worksheet, row: u32, col: u16, value: &str, report: &mut Report) -> Result<()> {
    let (v, t, c) = export_xlsx::clean(value);
    report.truncated += usize::from(t);
    report.cleaned += usize::from(c);
    s.write_string(row, col, &v).map_err(err)?;
    Ok(())
}
fn sheet(book: &mut Workbook, name: &str, headers: &[&str]) -> Result<usize> {
    let s = book.add_worksheet_with_constant_memory().map_err(err)?;
    s.set_name(name).map_err(err)?;
    s.set_freeze_panes(1, 0).map_err(err)?;
    for (c, h) in headers.iter().enumerate() {
        s.write_string_with_format(0, c as u16, *h, &Format::new().set_bold())
            .map_err(err)?;
        s.set_column_width(c as u16, if c == 0 { 24. } else { 30. })
            .map_err(err)?;
    }
    Ok(book.worksheets().len() - 1)
}
fn finish_sheet(book: &mut Workbook, index: usize, rows: u32, cols: u16) -> Result<()> {
    book.worksheet_from_index(index)
        .map_err(err)?
        .autofilter(0, 0, rows, cols - 1)
        .map_err(err)?;
    Ok(())
}
pub struct Context<'a> {
    pub local: bool,
    pub cache: &'a Cache,
    pub existing: Vec<Arc<EnumResult>>,
}
pub fn export(
    meta: &FileMeta,
    req: &Request,
    out: &Path,
    context: Context<'_>,
    cancel: &AtomicBool,
    progress: impl Fn(usize, &str) + Sync,
) -> Result<Report> {
    let Context {
        local,
        cache,
        existing,
    } = context;
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    if !out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
        || !(50..=5000).contains(&req.cap)
        || meta.columns.len() > 1_048_575
    {
        return Err(failure(
            "invalid_request",
            "Invalid dictionary export options",
        ));
    }
    if req.include_sample {
        sample::selected(&req.sample, sample::fields(meta)?.len())?;
        sample::plan(&[], &req.sample.mode, req.sample.n)?;
    }
    let catalog = enums::catalog(meta)?;
    if req.manual_columns.iter().any(|&r| r >= catalog.len()) {
        return Err(failure("invalid_request", "Invalid manual column"));
    }
    let mut report = Report {
        path: out.to_string_lossy().into(),
        ..Default::default()
    };
    let results: BTreeMap<usize, Arc<EnumResult>> = if req.existing_only {
        existing.into_iter().map(|r| (r.column.root, r)).collect()
    } else {
        let chosen: Vec<_> = catalog
            .iter()
            .filter(|c| c.candidate || req.manual_columns.contains(&c.root))
            .map(|c| c.root)
            .collect();
        if chosen.is_empty() {
            BTreeMap::new()
        } else {
            let _utc = display_time::Scope::new(false);
            progress(0, "enums");
            let result = enums::analyze(
                meta,
                &enums::Options {
                    columns: chosen,
                    cap: req.cap,
                    mode: "exact".into(),
                    k: 3,
                    force: false,
                },
                cancel,
                cache,
                |_| {},
                |p| progress(p.done, "enums"),
            )?;
            sample::check(cancel)?;
            report.warnings = result.warnings;
            result
                .results
                .into_iter()
                .map(|r| (r.column.root, Arc::new(r)))
                .collect()
        }
    };
    let _time = display_time::Scope::new(local);
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    let mut book = Workbook::new();
    book.set_tempdir(parent).map_err(err)?;
    let index = sheet(
        &mut book,
        "列清单",
        &[
            "序号",
            "列名",
            "逻辑类型",
            "物理类型",
            "可空",
            "null 数",
            "null 率",
            "最小值",
            "最大值",
            "distinct",
            "压缩方式",
            "数据编码",
            "压缩大小（字节）",
            "占文件比例",
            "统计状态",
            "说明",
        ],
    )?;
    let mut columns = meta.columns.clone();
    display_time::columns(&mut columns, local);
    for (i, c) in columns.iter().enumerate() {
        sample::check(cancel)?;
        let r = catalog
            .iter()
            .find(|r| r.leaves.contains(&c.index))
            .and_then(|c| results.get(&c.root));
        let distinct = r
            .filter(|r| !r.column.nested)
            .map(|r| match r.status.as_str() {
                "complete" => format!(
                    "{}{}",
                    if r.exact { "" } else { "样本 " },
                    r.distinct.unwrap_or(0)
                ),
                "high" => format!(">{}{}", r.cap, if r.exact { "" } else { "（样本）" }),
                _ => scope(r),
            })
            .unwrap_or_else(|| "—（未分析）".into());
        let values = [
            (i + 1).to_string(),
            c.path.clone(),
            c.logical_type.clone(),
            c.physical_type.clone(),
            if c.nullable { "是" } else { "否" }.into(),
            c.null_count.clone().unwrap_or_else(|| "—".into()),
            c.null_rate
                .map(|n| format!("{:.2}%", n * 100.))
                .unwrap_or_else(|| "—".into()),
            c.min
                .as_ref()
                .map(|s| format!("{}{s}", if c.min_max_truncated { "≈ " } else { "" }))
                .unwrap_or_else(|| "—".into()),
            c.max
                .as_ref()
                .map(|s| format!("{}{s}", if c.min_max_truncated { "≈ " } else { "" }))
                .unwrap_or_else(|| "—".into()),
            distinct,
            c.compression.clone(),
            c.encodings.clone(),
            c.compressed_bytes.clone(),
            format!("{:.2}%", c.compressed_ratio * 100.),
            if c.stats_complete {
                "完整"
            } else {
                "统计不完整"
            }
            .into(),
            c.notes
                .iter()
                .map(|s| note(s))
                .collect::<Vec<_>>()
                .join("；"),
        ];
        let s = book.worksheet_from_index(index).map_err(err)?;
        for (j, v) in values.iter().enumerate() {
            text(s, i as u32 + 1, j as u16, v, &mut report)?;
        }
    }
    finish_sheet(&mut book, index, columns.len() as u32, 16)?;
    progress(columns.len(), "columns");
    let headers = ["列名", "值", "空值", "次数", "占比", "结果范围/说明"];
    let mut page = 1;
    let mut index = sheet(&mut book, "枚举值", &headers)?;
    let mut row = 0u32;
    let builder = sample::builder(meta)?;
    for col in catalog {
        let result = results.get(&col.root);
        let values = if let Some(r) = result
            && r.status == "complete"
        {
            r.values.len()
        } else {
            1
        };
        for i in 0..values {
            sample::check(cancel)?;
            if row == 1_048_575 {
                finish_sheet(&mut book, index, row, 6)?;
                page += 1;
                index = sheet(&mut book, &format!("枚举值_{page}"), &headers)?;
                row = 0;
            }
            row += 1;
            let s = book.worksheet_from_index(index).map_err(err)?;
            text(s, row, 0, &col.name, &mut report)?;
            if let Some(r) = result {
                let note = scope(r);
                text(s, row, 5, &note, &mut report)?;
                if r.status == "complete" {
                    let v = &r.values[i];
                    if let Some(value) = &v.value {
                        text(
                            s,
                            row,
                            1,
                            &display_time::enum_text(
                                value,
                                builder.schema().field(col.root).data_type(),
                            ),
                            &mut report,
                        )?;
                    } else {
                        text(s, row, 1, "<NULL>", &mut report)?;
                    }
                    s.write_boolean(row, 2, v.value.is_none()).map_err(err)?;
                    let count = v.count.parse::<u64>().map_err(err)?;
                    if count < 1_000_000_000_000_000 {
                        s.write_number_with_format(
                            row,
                            3,
                            count as f64,
                            &Format::new().set_num_format("0"),
                        )
                        .map_err(err)?;
                    } else {
                        text(s, row, 3, &v.count, &mut report)?;
                        report.precision_text += 1;
                    }
                    let total = r.scope_rows.parse::<u64>().map_err(err)?;
                    if total > 0 {
                        s.write_number_with_format(
                            row,
                            4,
                            count as f64 / total as f64,
                            &Format::new().set_num_format("0.00%"),
                        )
                        .map_err(err)?;
                    }
                }
            } else {
                text(
                    s,
                    row,
                    5,
                    "未分析（未选择手动分析或没有已有结果）",
                    &mut report,
                )?;
            }
        }
    }
    finish_sheet(&mut book, index, row, 6)?;
    progress(row as usize, "enum_sheet");
    if req.include_sample {
        let sample_report =
            export_xlsx::write_sample_sheet(&mut book, meta, &req.sample, cancel, |n, _| {
                progress(n, "sample")
            })?;
        report.rows = sample_report.rows;
        report.columns = sample_report.columns;
        report.truncated += sample_report.truncated;
        report.cleaned += sample_report.cleaned;
        report.precision_text += sample_report.precision_text;
        report.empty_strings += sample_report.empty_strings;
    }
    sample::check(cancel)?;
    progress(0, "packaging");
    book.save_to_writer(tmp.as_file_mut()).map_err(err)?;
    tmp.as_file().sync_all().map_err(err)?;
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    tmp.persist(out).map_err(err)?;
    Ok(report)
}
