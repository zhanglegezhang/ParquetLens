use crate::{
    format::{self, Kind, Scalar},
    sample::{Result, parse_error},
};
use arrow::{array::*, datatypes::*};
use num_bigint::BigInt;
use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub enum Cell {
    Null,
    Text(String),
    Number(String),
    Bool(bool),
    Decimal(String, u8, i8),
    Date(String),
    Timestamp(String, bool, u32),
}
impl Cell {
    pub fn text(self) -> Option<String> {
        match self {
            Self::Null => None,
            Self::Bool(v) => Some(v.to_string()),
            Self::Timestamp(v, utc, _) => {
                Some(format!("{v} {}", if utc { "UTC" } else { "无时区" }))
            }
            Self::Text(v) | Self::Number(v) | Self::Decimal(v, ..) | Self::Date(v) => Some(v),
        }
    }
    fn json(self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(v) => Value::Bool(v),
            Self::Number(v) => {
                let digits = v
                    .trim_start_matches('-')
                    .replace('.', "")
                    .trim_start_matches('0')
                    .len();
                if digits <= 15
                    && let Ok(n) = serde_json::from_str::<Value>(&v)
                    && !n.to_string().contains(['e', 'E'])
                {
                    return n;
                }
                Value::String(v)
            }
            other => Value::String(other.text().unwrap_or_default()),
        }
    }
}
fn formatted(v: Scalar, k: Kind) -> Result<String> {
    format::display(&v, &k, false)
        .ok_or_else(|| parse_error("Value outside supported date/time range"))
}
fn unit(u: &TimeUnit) -> u32 {
    match u {
        TimeUnit::Second => 0,
        TimeUnit::Millisecond => 3,
        TimeUnit::Microsecond => 6,
        TimeUnit::Nanosecond => 9,
    }
}
fn json_at(a: &dyn Array, r: usize) -> Result<Value> {
    if a.is_null(r) {
        return Ok(Value::Null);
    }
    match a.data_type() {
        DataType::Struct(_) => {
            let s = a.as_any().downcast_ref::<StructArray>().unwrap();
            let mut map = Map::new();
            for (f, col) in s.fields().iter().zip(s.columns()) {
                if map.contains_key(f.name()) {
                    return Err(parse_error(
                        "Duplicate struct field names cannot be represented losslessly as JSON",
                    ));
                }
                map.insert(f.name().clone(), json_at(col.as_ref(), r)?);
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
            let entries = a.as_any().downcast_ref::<MapArray>().unwrap().value(r);
            let mut result = Vec::new();
            for i in 0..entries.len() {
                result.push(serde_json::json!({"key":json_at(entries.column(0).as_ref(),i)?,"value":json_at(entries.column(1).as_ref(),i)?}));
            }
            Ok(Value::Array(result))
        }
        _ => Ok(cell(a, r, false)?.json()),
    }
}
fn json_list(a: &dyn Array) -> Result<Value> {
    Ok(Value::Array(
        (0..a.len()).map(|i| json_at(a, i)).collect::<Result<_>>()?,
    ))
}
pub fn cell(a: &dyn Array, r: usize, uuid: bool) -> Result<Cell> {
    if a.is_null(r) {
        return Ok(Cell::Null);
    }
    macro_rules! primitive {
        ($t:ty) => {
            a.as_any()
                .downcast_ref::<PrimitiveArray<$t>>()
                .unwrap()
                .value(r)
        };
    }
    let text = match a.data_type() {
        DataType::Null => return Ok(Cell::Null),
        DataType::Boolean => {
            return Ok(Cell::Bool(
                a.as_any().downcast_ref::<BooleanArray>().unwrap().value(r),
            ));
        }
        DataType::Int8 => primitive!(Int8Type).to_string(),
        DataType::Int16 => primitive!(Int16Type).to_string(),
        DataType::Int32 => primitive!(Int32Type).to_string(),
        DataType::Int64 => primitive!(Int64Type).to_string(),
        DataType::UInt8 => primitive!(UInt8Type).to_string(),
        DataType::UInt16 => primitive!(UInt16Type).to_string(),
        DataType::UInt32 => primitive!(UInt32Type).to_string(),
        DataType::UInt64 => primitive!(UInt64Type).to_string(),
        DataType::Float32 => formatted(
            Scalar::Float(primitive!(Float32Type) as f64),
            Kind::Float(true),
        )?,
        DataType::Float64 => formatted(Scalar::Float(primitive!(Float64Type)), Kind::Float(false))?,
        DataType::Decimal32(p, s) => {
            return Ok(Cell::Decimal(
                format::decimal(&BigInt::from(primitive!(Decimal32Type)), *s as i32).unwrap(),
                *p,
                *s,
            ));
        }
        DataType::Decimal64(p, s) => {
            return Ok(Cell::Decimal(
                format::decimal(&BigInt::from(primitive!(Decimal64Type)), *s as i32).unwrap(),
                *p,
                *s,
            ));
        }
        DataType::Decimal128(p, s) => {
            return Ok(Cell::Decimal(
                format::decimal(&BigInt::from(primitive!(Decimal128Type)), *s as i32).unwrap(),
                *p,
                *s,
            ));
        }
        DataType::Decimal256(p, s) => {
            return Ok(Cell::Decimal(
                format::decimal(
                    &primitive!(Decimal256Type)
                        .to_string()
                        .parse()
                        .map_err(parse_error)?,
                    *s as i32,
                )
                .unwrap(),
                *p,
                *s,
            ));
        }
        DataType::Utf8 => {
            return Ok(Cell::Text(
                a.as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap()
                    .value(r)
                    .into(),
            ));
        }
        DataType::LargeUtf8 => {
            return Ok(Cell::Text(
                a.as_any()
                    .downcast_ref::<LargeStringArray>()
                    .unwrap()
                    .value(r)
                    .into(),
            ));
        }
        DataType::Utf8View => {
            return Ok(Cell::Text(
                a.as_any()
                    .downcast_ref::<StringViewArray>()
                    .unwrap()
                    .value(r)
                    .into(),
            ));
        }
        DataType::Date32 => {
            return Ok(Cell::Date(formatted(
                Scalar::Int(primitive!(Date32Type).into()),
                Kind::Date,
            )?));
        }
        DataType::Date64 => {
            return Ok(Cell::Date(formatted(
                Scalar::Int(primitive!(Date64Type).div_euclid(86400000).into()),
                Kind::Date,
            )?));
        }
        DataType::Timestamp(u, tz) => {
            let n = match u {
                TimeUnit::Second => primitive!(TimestampSecondType),
                TimeUnit::Millisecond => primitive!(TimestampMillisecondType),
                TimeUnit::Microsecond => primitive!(TimestampMicrosecondType),
                TimeUnit::Nanosecond => primitive!(TimestampNanosecondType),
            };
            let digits = unit(u);
            let mut text =
                format::timestamp(n, digits).ok_or_else(|| parse_error("Invalid timestamp"))?;
            if digits == 0 {
                text.truncate(text.len() - 2);
            }
            if tz.is_some() && crate::display_time::enabled() {
                return Ok(Cell::Text(crate::display_time::local_text(&text)));
            }
            return Ok(Cell::Timestamp(text, tz.is_some(), digits));
        }
        DataType::Time32(u) => {
            let n = match u {
                TimeUnit::Second => primitive!(Time32SecondType),
                TimeUnit::Millisecond => primitive!(Time32MillisecondType),
                _ => return Err(parse_error("Invalid Time32 unit")),
            };
            return Ok(Cell::Text(formatted(
                Scalar::Int(n.into()),
                Kind::Time(unit(u)),
            )?));
        }
        DataType::Time64(u) => {
            let n = match u {
                TimeUnit::Microsecond => primitive!(Time64MicrosecondType),
                TimeUnit::Nanosecond => primitive!(Time64NanosecondType),
                _ => return Err(parse_error("Invalid Time64 unit")),
            };
            return Ok(Cell::Text(formatted(
                Scalar::Int(n.into()),
                Kind::Time(unit(u)),
            )?));
        }
        DataType::Binary
        | DataType::LargeBinary
        | DataType::FixedSizeBinary(_)
        | DataType::BinaryView => {
            let bytes = match a.data_type() {
                DataType::Binary => a.as_any().downcast_ref::<BinaryArray>().unwrap().value(r),
                DataType::LargeBinary => a
                    .as_any()
                    .downcast_ref::<LargeBinaryArray>()
                    .unwrap()
                    .value(r),
                DataType::BinaryView => a
                    .as_any()
                    .downcast_ref::<BinaryViewArray>()
                    .unwrap()
                    .value(r),
                _ => a
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .unwrap()
                    .value(r),
            };
            return Ok(Cell::Text(formatted(
                Scalar::Bytes(bytes.into()),
                if uuid { Kind::Uuid } else { Kind::Binary },
            )?));
        }
        DataType::Dictionary(key, _) => {
            macro_rules! dictionary {
                ($t:ty) => {{
                    let d = a.as_any().downcast_ref::<DictionaryArray<$t>>().unwrap();
                    return cell(d.values().as_ref(), d.keys().value(r) as usize, uuid);
                }};
            }
            match key.as_ref() {
                DataType::Int8 => dictionary!(Int8Type),
                DataType::Int16 => dictionary!(Int16Type),
                DataType::Int32 => dictionary!(Int32Type),
                DataType::Int64 => dictionary!(Int64Type),
                DataType::UInt8 => dictionary!(UInt8Type),
                DataType::UInt16 => dictionary!(UInt16Type),
                DataType::UInt32 => dictionary!(UInt32Type),
                DataType::UInt64 => dictionary!(UInt64Type),
                _ => return Err(parse_error("Unsupported dictionary key")),
            }
        }
        DataType::List(_)
        | DataType::LargeList(_)
        | DataType::FixedSizeList(_, _)
        | DataType::Struct(_)
        | DataType::Map(_, _) => return Ok(Cell::Text(json_at(a, r)?.to_string())),
        other => return Err(parse_error(format!("Unsupported sample type: {other}"))),
    };
    Ok(Cell::Number(text))
}
