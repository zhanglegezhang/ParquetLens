use crate::sample::{Result, parse_error};
use arrow::{array::*, datatypes::*};
use serde_json::{Map, Value};

pub fn dictionary(a: &dyn Array, r: usize) -> Result<(&dyn Array, usize)> {
    if let DataType::Dictionary(k, _) = a.data_type() {
        macro_rules! d {
            ($t:ty) => {{
                let v = a.as_any().downcast_ref::<DictionaryArray<$t>>().unwrap();
                return Ok((v.values().as_ref(), v.keys().value(r) as usize));
            }};
        }
        match k.as_ref() {
            DataType::Int8 => d!(Int8Type),
            DataType::Int16 => d!(Int16Type),
            DataType::Int32 => d!(Int32Type),
            DataType::Int64 => d!(Int64Type),
            DataType::UInt8 => d!(UInt8Type),
            DataType::UInt16 => d!(UInt16Type),
            DataType::UInt32 => d!(UInt32Type),
            DataType::UInt64 => d!(UInt64Type),
            _ => return Err(parse_error("Unsupported dictionary key")),
        }
    }
    Ok((a, r))
}
pub fn bytes(a: &dyn Array, r: usize) -> Option<&[u8]> {
    match a.data_type() {
        DataType::Utf8 => Some(
            a.as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(r)
                .as_bytes(),
        ),
        DataType::LargeUtf8 => Some(
            a.as_any()
                .downcast_ref::<LargeStringArray>()
                .unwrap()
                .value(r)
                .as_bytes(),
        ),
        DataType::Utf8View => Some(
            a.as_any()
                .downcast_ref::<StringViewArray>()
                .unwrap()
                .value(r)
                .as_bytes(),
        ),
        DataType::Binary => Some(a.as_any().downcast_ref::<BinaryArray>().unwrap().value(r)),
        DataType::LargeBinary => Some(
            a.as_any()
                .downcast_ref::<LargeBinaryArray>()
                .unwrap()
                .value(r),
        ),
        DataType::BinaryView => Some(
            a.as_any()
                .downcast_ref::<BinaryViewArray>()
                .unwrap()
                .value(r),
        ),
        DataType::FixedSizeBinary(_) => Some(
            a.as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .unwrap()
                .value(r),
        ),
        _ => None,
    }
}
fn part(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(data);
}
pub fn nested(a: &dyn Array, r: usize, out: &mut Vec<u8>) -> Result<()> {
    if out.len() > 16 * 1024 * 1024 {
        return Err(crate::meta::failure(
            "resource_limit",
            "Nested key exceeds 16 MiB",
        ));
    }
    if a.is_null(r) {
        out.push(0);
        return Ok(());
    }
    let (a, r) = dictionary(a, r)?;
    if a.is_null(r) {
        out.push(0);
        return Ok(());
    }
    out.push(1);
    if let Some(b) = bytes(a, r) {
        if out.len().saturating_add(b.len() + 8) > 16 * 1024 * 1024 {
            return Err(crate::meta::failure(
                "resource_limit",
                "Nested key exceeds 16 MiB",
            ));
        }
        part(out, b);
        return Ok(());
    }
    match a.data_type() {
        DataType::Struct(_) => {
            let a = a.as_any().downcast_ref::<StructArray>().unwrap();
            for c in a.columns() {
                nested(c.as_ref(), r, out)?;
            }
        }
        DataType::List(_) => list(
            a.as_any()
                .downcast_ref::<ListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
            out,
        )?,
        DataType::LargeList(_) => list(
            a.as_any()
                .downcast_ref::<LargeListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
            out,
        )?,
        DataType::FixedSizeList(_, _) => list(
            a.as_any()
                .downcast_ref::<FixedSizeListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
            out,
        )?,
        DataType::Map(_, _) => list(
            &a.as_any().downcast_ref::<MapArray>().unwrap().value(r),
            out,
        )?,
        _ => {
            let s = crate::value::cell(a, r, false)?.text().unwrap_or_default();
            part(out, s.as_bytes());
        }
    }
    Ok(())
}
fn list(a: &dyn Array, out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(&(a.len() as u64).to_le_bytes());
    for r in 0..a.len() {
        nested(a, r, out)?;
    }
    Ok(())
}
// Full binary text is used here, unlike M2's deliberately abbreviated preview.
pub fn display(a: &dyn Array, r: usize, uuid: bool) -> Result<String> {
    let (a, r) = dictionary(a, r)?;
    if matches!(
        a.data_type(),
        DataType::Binary
            | DataType::LargeBinary
            | DataType::BinaryView
            | DataType::FixedSizeBinary(_)
    ) && !uuid
    {
        let b = bytes(a, r).unwrap();
        return Ok(format!(
            "<binary {} bytes> {}",
            b.len(),
            b.iter().map(|v| format!("{v:02x}")).collect::<String>()
        ));
    }
    if matches!(
        a.data_type(),
        DataType::Struct(_)
            | DataType::List(_)
            | DataType::LargeList(_)
            | DataType::FixedSizeList(_, _)
            | DataType::Map(_, _)
    ) {
        return Ok(json(a, r)?.to_string());
    }
    Ok(crate::value::cell(a, r, uuid)?.text().unwrap_or_default())
}
fn json(a: &dyn Array, r: usize) -> Result<Value> {
    if a.is_null(r) {
        return Ok(Value::Null);
    }
    let (a, r) = dictionary(a, r)?;
    if a.is_null(r) {
        return Ok(Value::Null);
    }
    match a.data_type() {
        DataType::Struct(_) => {
            let a = a.as_any().downcast_ref::<StructArray>().unwrap();
            let mut map = Map::new();
            for (f, c) in a.fields().iter().zip(a.columns()) {
                if map.contains_key(f.name()) {
                    return Err(parse_error("Duplicate struct names"));
                }
                map.insert(f.name().clone(), json(c.as_ref(), r)?);
            }
            Ok(Value::Object(map))
        }
        DataType::List(_) => json_list(
            a.as_any()
                .downcast_ref::<ListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
        ),
        DataType::LargeList(_) => json_list(
            a.as_any()
                .downcast_ref::<LargeListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
        ),
        DataType::FixedSizeList(_, _) => json_list(
            a.as_any()
                .downcast_ref::<FixedSizeListArray>()
                .unwrap()
                .value(r)
                .as_ref(),
        ),
        DataType::Map(_, _) => {
            let v = a.as_any().downcast_ref::<MapArray>().unwrap().value(r);
            Ok(Value::Array((0..v.len()).map(|i|Ok(serde_json::json!({"key":json(v.column(0).as_ref(),i)?,"value":json(v.column(1).as_ref(),i)?}))).collect::<Result<_>>()?))
        }
        DataType::Boolean => Ok(Value::Bool(
            a.as_any().downcast_ref::<BooleanArray>().unwrap().value(r),
        )),
        _ => {
            let text = display(a, r, false)?;
            if matches!(
                a.data_type(),
                DataType::Int8
                    | DataType::Int16
                    | DataType::Int32
                    | DataType::UInt8
                    | DataType::UInt16
                    | DataType::UInt32
            ) {
                return serde_json::from_str(&text).map_err(parse_error);
            }
            Ok(Value::String(text))
        }
    }
}
fn json_list(a: &dyn Array) -> Result<Value> {
    Ok(Value::Array(
        (0..a.len()).map(|i| json(a, i)).collect::<Result<_>>()?,
    ))
}
