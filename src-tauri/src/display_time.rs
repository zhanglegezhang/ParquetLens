use chrono::{Local, NaiveDateTime, TimeZone};
use std::cell::Cell;
thread_local! {static LOCAL:Cell<bool>=const{Cell::new(false)};}
pub struct Scope(bool);
impl Scope {
    pub fn new(local: bool) -> Self {
        Self(LOCAL.with(|v| v.replace(local)))
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        LOCAL.with(|v| v.set(self.0));
    }
}
pub fn enabled() -> bool {
    LOCAL.with(Cell::get)
}
pub fn local_text(text: &str) -> String {
    let raw = text.strip_suffix(" UTC").unwrap_or(text);
    let Ok(dt) = NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f") else {
        return text.into();
    };
    let local = Local.from_utc_datetime(&dt);
    let fraction = raw
        .split_once('.')
        .map(|(_, f)| format!(".{f}"))
        .unwrap_or_default();
    format!(
        "{}{fraction} {}",
        local.format("%Y-%m-%d %H:%M:%S"),
        local.format("%:z")
    )
}
pub fn columns(cols: &mut [crate::meta::ColumnInfo], local: bool) {
    if !local {
        return;
    }
    for c in cols {
        if c.logical_type.starts_with("Timestamp(") && c.logical_type.contains("UTC") {
            c.min = c.min.as_ref().map(|s| local_text(s));
            c.max = c.max.as_ref().map(|s| local_text(s));
        }
    }
}
pub fn enum_text(value: &str, data_type: &arrow::datatypes::DataType) -> String {
    use arrow::datatypes::DataType;
    fn convert(v: &mut serde_json::Value, t: &DataType) {
        match t {
            DataType::Dictionary(_, t) => convert(v, t),
            DataType::Timestamp(_, Some(_)) => {
                if let Some(s) = v.as_str() {
                    *v = serde_json::Value::String(local_text(s));
                }
            }
            DataType::Struct(fields) => {
                if let Some(obj) = v.as_object_mut() {
                    for f in fields {
                        if let Some(v) = obj.get_mut(f.name()) {
                            convert(v, f.data_type());
                        }
                    }
                }
            }
            DataType::List(f) | DataType::LargeList(f) | DataType::FixedSizeList(f, _) => {
                if let Some(a) = v.as_array_mut() {
                    for v in a {
                        convert(v, f.data_type());
                    }
                }
            }
            DataType::Map(f, _) => {
                if let DataType::Struct(fields) = f.data_type()
                    && let Some(a) = v.as_array_mut()
                {
                    for pair in a {
                        for (i, key) in ["key", "value"].iter().enumerate() {
                            if let Some(v) = pair.get_mut(*key) {
                                convert(v, fields[i].data_type());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !enabled() {
        return value.into();
    }
    if matches!(data_type, DataType::Timestamp(_, Some(_))) {
        return local_text(value);
    }
    if let DataType::Dictionary(_, t) = data_type {
        return enum_text(value, t);
    }
    if !matches!(
        data_type,
        DataType::Struct(_)
            | DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(_, _)
            | DataType::Map(_, _)
    ) {
        return value.into();
    }
    if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(value) {
        convert(&mut v, data_type);
        return v.to_string();
    }
    value.into()
}
