use crate::{
    display_time,
    enums::EnumResult,
    export_xlsx::Report,
    meta::{FileMeta, failure},
    sample::{self, Request, Result},
    value,
};
use std::{io::Write, path::Path, sync::atomic::AtomicBool};
fn err(e: impl ToString) -> crate::files::OpenError {
    failure("export_failed", e)
}
pub fn row(
    out: &mut impl Write,
    values: &[(Option<String>, bool)],
    protect: bool,
) -> Result<usize> {
    let mut protected = 0;
    for (i, (v, text)) in values.iter().enumerate() {
        if i > 0 {
            out.write_all(b",").map_err(err)?;
        }
        if let Some(v) = v {
            out.write_all(b"\"").map_err(err)?;
            if protect && *text && v.trim_start().starts_with(['=', '+', '-', '@']) {
                out.write_all(b"'").map_err(err)?;
                protected += 1;
            }
            // Stream escaping rather than allocating a second copy of long text.
            let mut start = 0;
            for (pos, ch) in v.char_indices() {
                if ch == '"' {
                    out.write_all(&v.as_bytes()[start..pos]).map_err(err)?;
                    out.write_all(b"\"\"").map_err(err)?;
                    start = pos + 1;
                }
            }
            out.write_all(&v.as_bytes()[start..]).map_err(err)?;
            out.write_all(b"\"").map_err(err)?;
        }
    }
    out.write_all(b"\r\n").map_err(err)?;
    Ok(protected)
}
fn staging(meta: &FileMeta, out: &Path, cancel: &AtomicBool) -> Result<tempfile::NamedTempFile> {
    sample::unchanged(meta)?;
    sample::check(cancel)?;
    if !out
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
    {
        return Err(failure("invalid_request", "Output must be .csv"));
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    tempfile::NamedTempFile::new_in(parent).map_err(err)
}
fn finish(
    temp: tempfile::NamedTempFile,
    meta: &FileMeta,
    out: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    temp.as_file().sync_all().map_err(err)?;
    sample::check(cancel)?;
    sample::unchanged(meta)?;
    temp.persist(out).map_err(err)?;
    Ok(())
}
pub fn sample_export(
    meta: &FileMeta,
    req: &Request,
    out: &Path,
    protect: bool,
    cancel: &AtomicBool,
    mut progress: impl FnMut(usize),
) -> Result<Report> {
    let mut temp = staging(meta, out, cancel)?;
    let fields = sample::fields(meta)?;
    let cols = sample::selected(req, fields.len())?;
    sample::plan(&[], &req.mode, req.n)?;
    let mut report = Report {
        columns: cols.len(),
        path: out.to_string_lossy().into(),
        ..Default::default()
    };
    {
        let mut w = std::io::BufWriter::new(temp.as_file_mut());
        w.write_all(b"\xef\xbb\xbf").map_err(err)?;
        report.protected += row(
            &mut w,
            &cols
                .iter()
                .map(|&i| (Some(fields[i].name.clone()), true))
                .collect::<Vec<_>>(),
            protect,
        )?;
        sample::scan(meta, req, 0, req.n, cancel, |batch, done| {
            for r in 0..batch.num_rows() {
                sample::check(cancel)?;
                let mut values = Vec::with_capacity(cols.len());
                for (i, a) in batch.columns().iter().enumerate() {
                    let cell = value::cell(a.as_ref(), r, fields[cols[i]].uuid)?;
                    let is_text = matches!(cell, value::Cell::Text(_));
                    values.push((cell.text(), is_text));
                }
                report.protected += row(&mut w, &values, protect)?;
            }
            report.rows = done + batch.num_rows();
            progress(report.rows);
            Ok(())
        })?;
        w.flush().map_err(err)?;
    }
    finish(temp, meta, out, cancel)?;
    Ok(report)
}
pub fn enum_export(
    meta: &FileMeta,
    result: &EnumResult,
    out: &Path,
    protect: bool,
    cancel: &AtomicBool,
) -> Result<Report> {
    if !matches!(result.status.as_str(), "complete" | "high") {
        return Err(failure("invalid_request", "Enumeration is incomplete"));
    }
    let mut temp = staging(meta, out, cancel)?;
    let builder = sample::builder(meta)?;
    let typ = builder.schema().field(result.column.root).data_type();
    let mut report = Report {
        columns: 6,
        path: out.to_string_lossy().into(),
        ..Default::default()
    };
    {
        let mut w = std::io::BufWriter::new(temp.as_file_mut());
        w.write_all(b"\xef\xbb\xbf").map_err(err)?;
        row(
            &mut w,
            &["列名", "值", "空值", "次数", "占比", "结果范围/说明"]
                .iter()
                .map(|s| (Some((*s).into()), true))
                .collect::<Vec<_>>(),
            protect,
        )?;
        let scope = crate::dictionary::scope(result);
        if result.status == "high" {
            report.protected += row(
                &mut w,
                &[
                    (Some(result.column.name.clone()), true),
                    (None, false),
                    (None, false),
                    (None, false),
                    (None, false),
                    (Some(scope), true),
                ],
                protect,
            )?;
            report.rows = 1;
        } else {
            for offset in (0..result.values.len()).step_by(500) {
                let p = crate::enum_store::page(result, offset, 500, "", "count", true)?;
                for v in p.rows {
                    sample::check(cancel)?;
                    let total = result.scope_rows.parse::<f64>().unwrap_or(0.);
                    let ratio = if total > 0. {
                        Some((v.count.parse::<f64>().unwrap_or(0.) / total).to_string())
                    } else {
                        None
                    };
                    report.protected += row(
                        &mut w,
                        &[
                            (Some(result.column.name.clone()), true),
                            (
                                v.value.as_ref().map(|s| display_time::enum_text(s, typ)),
                                true,
                            ),
                            (Some(v.value.is_none().to_string()), false),
                            (Some(v.count), false),
                            (ratio, false),
                            (Some(scope.clone()), true),
                        ],
                        protect,
                    )?;
                    report.rows += 1;
                }
            }
        }
        w.flush().map_err(err)?;
    }
    finish(temp, meta, out, cancel)?;
    Ok(report)
}
