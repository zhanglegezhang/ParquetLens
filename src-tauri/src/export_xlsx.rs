use crate::{
    meta::{FileMeta, failure},
    sample::{self, Request, Result},
    value::{self, Cell},
};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};
use serde::Serialize;
use std::{path::Path, sync::atomic::AtomicBool};

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub rows: usize,
    pub columns: usize,
    pub truncated: usize,
    pub cleaned: usize,
    pub precision_text: usize,
    pub empty_strings: usize,
    pub path: String,
    pub protected: usize,
    pub warnings: Vec<String>,
}
fn err(e: impl ToString) -> crate::files::OpenError {
    failure("export_failed", e)
}
// Count UTF-16 units conservatively so surrogate pairs are never cut in half.
pub fn clean(text: &str) -> (String, bool, bool) {
    let clean: String = text
        .chars()
        .filter(|&c| {
            matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
        })
        .collect();
    let changed = clean != text;
    if clean.encode_utf16().count() <= 32767 {
        return (clean, false, changed);
    }
    let suffix = "…(truncated)";
    let cap = 32767 - suffix.encode_utf16().count();
    let mut count = 0;
    let mut result = String::new();
    for ch in clean.chars() {
        count += ch.len_utf16();
        if count > cap {
            break;
        }
        result.push(ch);
    }
    result.push_str(suffix);
    (result, true, changed)
}
fn text(sheet: &mut Worksheet, r: u32, c: u16, s: &str, report: &mut Report) -> Result<()> {
    let (s, truncated, cleaned) = clean(s);
    report.truncated += usize::from(truncated);
    report.cleaned += usize::from(cleaned);
    if s.is_empty() {
        report.empty_strings += 1;
        // Excel's writer represents empty strings as blank cells. A text format
        // keeps the intended column semantics without inserting a formula/value.
        sheet
            .write_string_with_format(r, c, &s, &Format::new().set_num_format("@"))
            .map_err(err)?;
    } else {
        sheet.write_string(r, c, &s).map_err(err)?;
    }
    Ok(())
}
pub fn safe_number(s: &str) -> Option<(f64, String)> {
    let n = s.parse::<f64>().ok()?;
    if !n.is_finite()
        || (n == 0.0 && s.starts_with('-'))
        || (n != 0.0 && n.abs() < f64::MIN_POSITIVE)
    {
        return None;
    }
    let raw = s.trim_start_matches('-');
    let digits = raw.replace('.', "");
    if digits.trim_start_matches('0').len() > 15 || (!raw.contains('.') && n.abs() >= 1e15) {
        return None;
    }
    let places = raw.split_once('.').map_or(0, |(_, v)| v.len());
    if places > 30 {
        return None;
    }
    Some((
        n,
        if places == 0 {
            "0".into()
        } else {
            format!("0.{}", "#".repeat(places))
        },
    ))
}
fn date_serial(s: &str) -> Option<f64> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    if !(1900..=9999).contains(&date.year()) {
        return None;
    }
    let epoch = NaiveDate::from_ymd_opt(1899, 12, 31)?;
    let mut days = (date - epoch).num_days();
    if date >= NaiveDate::from_ymd_opt(1900, 3, 1)? {
        days += 1;
    }
    Some(days as f64)
}
fn write(sheet: &mut Worksheet, r: u32, c: u16, value: Cell, report: &mut Report) -> Result<()> {
    match value {
        Cell::Null => {}
        Cell::Bool(v) => {
            sheet.write_boolean(r, c, v).map_err(err)?;
        }
        Cell::Number(s) => {
            if let Some((v, fmt)) = safe_number(&s) {
                sheet
                    .write_number_with_format(r, c, v, &Format::new().set_num_format(fmt))
                    .map_err(err)?;
            } else {
                report.precision_text += 1;
                text(sheet, r, c, &s, report)?;
            }
        }
        Cell::Decimal(s, p, scale) => {
            if p <= 15 && (0..=30).contains(&scale) && s.parse::<f64>().is_ok_and(f64::is_finite) {
                let fmt = if scale == 0 {
                    "0".into()
                } else {
                    format!("0.{}", "0".repeat(scale as usize))
                };
                sheet
                    .write_number_with_format(
                        r,
                        c,
                        s.parse::<f64>().unwrap(),
                        &Format::new().set_num_format(fmt),
                    )
                    .map_err(err)?;
            } else {
                report.precision_text += 1;
                text(sheet, r, c, &s, report)?;
            }
        }
        Cell::Date(s) => {
            if let Some(v) = date_serial(&s) {
                sheet
                    .write_datetime_with_format(
                        r,
                        c,
                        &ExcelDateTime::from_serial_datetime(v).map_err(err)?,
                        &Format::new().set_num_format("yyyy-mm-dd"),
                    )
                    .map_err(err)?;
            } else {
                text(sheet, r, c, &s, report)?;
                report.precision_text += 1;
            }
        }
        Cell::Timestamp(s, utc, digits) => {
            let serial = if digits <= 3 {
                s.get(..10).and_then(date_serial).and_then(|day| {
                    let dt = NaiveDateTime::parse_from_str(
                        &s,
                        if digits == 0 {
                            "%Y-%m-%d %H:%M:%S"
                        } else {
                            "%Y-%m-%d %H:%M:%S%.f"
                        },
                    )
                    .ok()?;
                    let midnight = dt.date().and_hms_opt(0, 0, 0)?;
                    let ms = (dt - midnight).num_milliseconds();
                    let value = day + ms as f64 / 86400000.0;
                    (((value - day) * 86400000.0).round() as i64 == ms).then_some(value)
                })
            } else {
                None
            };
            if let Some(v) = serial {
                let fmt = format!(
                    "yyyy-mm-dd hh:mm:ss{}\" {}\"",
                    if digits == 0 { "" } else { ".000" },
                    if utc { "UTC" } else { "无时区" }
                );
                sheet
                    .write_datetime_with_format(
                        r,
                        c,
                        &ExcelDateTime::from_serial_datetime(v).map_err(err)?,
                        &Format::new().set_num_format(fmt),
                    )
                    .map_err(err)?;
            } else {
                report.precision_text += 1;
                text(
                    sheet,
                    r,
                    c,
                    &format!("{s} {}", if utc { "UTC" } else { "无时区" }),
                    report,
                )?;
            }
        }
        Cell::Text(s) => text(sheet, r, c, &s, report)?,
    }
    Ok(())
}
pub fn export(
    meta: &FileMeta,
    req: &Request,
    out: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, &str),
) -> Result<Report> {
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    if !out
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("xlsx"))
    {
        return Err(failure(
            "invalid_request",
            "Output must have .xlsx extension",
        ));
    }
    if out.canonicalize().ok() == Some(Path::new(&meta.path).to_path_buf()) {
        return Err(failure("invalid_request", "Cannot replace source"));
    }
    let fields = sample::fields(meta)?;
    let cols = sample::selected(req, fields.len())?;
    if cols.len() > 16384 || req.n > 1_048_575 {
        return Err(failure(
            "invalid_request",
            "Excel row/column limit exceeded",
        ));
    }
    sample::plan(&[], &req.mode, req.n)?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut staging = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
    let mut workbook = Workbook::new();
    workbook.set_tempdir(parent).map_err(err)?;
    let mut report = write_sample_sheet(&mut workbook, meta, req, cancel, &mut progress)?;
    report.path = out.to_string_lossy().into();
    sample::check(cancel)?;
    progress(report.rows, "packaging");
    workbook
        .save_to_writer(staging.as_file_mut())
        .map_err(err)?;
    staging.as_file().sync_all().map_err(err)?;
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    staging.persist(out).map_err(err)?;
    Ok(report)
}
pub fn write_sample_sheet(
    workbook: &mut Workbook,
    meta: &FileMeta,
    req: &Request,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize, &str),
) -> Result<Report> {
    let fields = sample::fields(meta)?;
    let cols = sample::selected(req, fields.len())?;
    if cols.len() > 16384 || req.n > 1_048_575 {
        return Err(failure(
            "invalid_request",
            "Excel row/column limit exceeded",
        ));
    }
    sample::plan(&[], &req.mode, req.n)?;
    let sheet = workbook.add_worksheet_with_constant_memory().map_err(err)?;
    sheet.set_name("样例").map_err(err)?;
    sheet.set_freeze_panes(1, 0).map_err(err)?;
    let header = Format::new().set_bold();
    let mut widths = Vec::new();
    let mut report = Report {
        columns: cols.len(),

        ..Default::default()
    };
    for (c, &i) in cols.iter().enumerate() {
        let (name, truncated, cleaned) = clean(&fields[i].name);
        report.truncated += usize::from(truncated);
        report.cleaned += usize::from(cleaned);
        sheet
            .write_string_with_format(0, c as u16, &name, &header)
            .map_err(err)?;
        widths.push(width(&name));
    }
    sample::scan(meta, req, 0, req.n, cancel, |batch, done| {
        for r in 0..batch.num_rows() {
            sample::check(cancel)?;
            for (c, a) in batch.columns().iter().enumerate() {
                let value = value::cell(a.as_ref(), r, fields[cols[c]].uuid)?;
                if let Some(s) = value.clone().text() {
                    widths[c] = widths[c].max(width(&s));
                }
                write(sheet, (done + r + 1) as u32, c as u16, value, &mut report)?;
            }
        }
        report.rows = done + batch.num_rows();
        progress(report.rows, "writing");
        Ok(())
    })?;
    sheet
        .autofilter(0, 0, report.rows as u32, (cols.len() - 1) as u16)
        .map_err(err)?;
    for (c, w) in widths.iter().enumerate() {
        sheet
            .set_column_width(c as u16, (w + 2).clamp(8, 60) as f64)
            .map_err(err)?;
    }
    Ok(report)
}

fn width(s: &str) -> usize {
    s.lines()
        .take(256)
        .map(|line| {
            line.chars()
                .take(60)
                .map(|c| if c.is_ascii() { 1 } else { 2 })
                .sum()
        })
        .max()
        .unwrap_or(0)
        .min(60)
}
