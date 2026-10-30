use crate::{
    enums::EnumResult,
    export_xlsx::{self, Report},
    meta::{FileMeta, failure},
    sample::{self, Result},
};
use rust_xlsxwriter::{Format, Workbook};
use std::{path::Path, sync::atomic::AtomicBool};
fn err(e: impl ToString) -> crate::files::OpenError {
    failure("export_failed", e)
}
pub fn export(
    meta: &FileMeta,
    result: &EnumResult,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<Report> {
    sample::unchanged(meta)?;
    sample::check(cancel)?;
    if !out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
        || !matches!(result.status.as_str(), "complete" | "high")
    {
        return Err(failure(
            "invalid_request",
            "Only complete or high-cardinality results can be exported to .xlsx",
        ));
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    let mut workbook = Workbook::new();
    workbook.set_tempdir(parent).map_err(err)?;
    let sheet = workbook.add_worksheet_with_constant_memory().map_err(err)?;
    sheet.set_name("枚举值").map_err(err)?;
    sheet.set_freeze_panes(1, 0).map_err(err)?;
    let bold = Format::new().set_bold();
    for (c, h) in ["列名", "值", "空值", "次数", "占比", "结果范围/说明"]
        .iter()
        .enumerate()
    {
        sheet
            .write_string_with_format(0, c as u16, *h, &bold)
            .map_err(err)?;
    }
    let mut report = Report {
        columns: 6,
        path: out.to_string_lossy().into(),
        ..Default::default()
    };
    let scope = if result.exact {
        format!("全量 {} 行", result.scope_rows)
    } else {
        format!(
            "采样结果，非全量；前 {} / {} 个 row group，{} / {} 行",
            result.scope_groups, result.file_groups, result.scope_rows, result.file_rows
        )
    };
    let note = if result.status == "high" {
        format!("基数 > {}；{scope}；未提供部分频数", result.cap)
    } else {
        scope
    };
    let values = crate::enum_store::page(result, 0, 500, "", "count", true)?;
    let all = if result.status == "high" {
        vec![crate::enums::Entry {
            value: Some(String::new()),
            count: String::new(),
        }]
    } else {
        let mut rows = values.rows;
        for offset in (500..result.values.len()).step_by(500) {
            rows.extend(crate::enum_store::page(result, offset, 500, "", "count", true)?.rows);
        }
        rows
    };
    let builder = sample::builder(meta)?;
    let mut all = all;
    for value in &mut all {
        if let Some(s) = &mut value.value {
            *s = crate::display_time::enum_text(
                s,
                builder.schema().field(result.column.root).data_type(),
            );
        }
    }
    for (i, value) in all.iter().enumerate() {
        sample::check(cancel)?;
        let row = i as u32 + 1;
        for (col, text) in [
            (0, result.column.name.as_str()),
            (1, value.value.as_deref().unwrap_or("<NULL>")),
            (5, note.as_str()),
        ] {
            let (text, truncated, cleaned) = export_xlsx::clean(text);
            report.truncated += usize::from(truncated);
            report.cleaned += usize::from(cleaned);
            if text.is_empty() {
                report.empty_strings += 1;
            }
            sheet.write_string(row, col, &text).map_err(err)?;
        }
        if result.status == "complete" {
            sheet
                .write_boolean(row, 2, value.value.is_none())
                .map_err(err)?;
            let count = value.count.parse::<u64>().map_err(err)?;
            if count < 1_000_000_000_000_000 {
                sheet
                    .write_number_with_format(
                        row,
                        3,
                        count as f64,
                        &Format::new().set_num_format("0"),
                    )
                    .map_err(err)?;
            } else {
                sheet.write_string(row, 3, &value.count).map_err(err)?;
                report.precision_text += 1;
            }
            let total = result.scope_rows.parse::<u64>().map_err(err)?;
            if total > 0 {
                sheet
                    .write_number_with_format(
                        row,
                        4,
                        count as f64 / total as f64,
                        &Format::new().set_num_format("0.00%"),
                    )
                    .map_err(err)?;
            }
        }
    }
    report.rows = all.len();
    sheet.autofilter(0, 0, all.len() as u32, 5).map_err(err)?;
    for (col, width) in [24., 60., 10., 20., 14., 60.].iter().enumerate() {
        sheet.set_column_width(col as u16, *width).map_err(err)?;
    }
    workbook.save_to_writer(temp.as_file_mut()).map_err(err)?;
    temp.as_file().sync_all().map_err(err)?;
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    temp.persist(out).map_err(err)?;
    Ok(report)
}
