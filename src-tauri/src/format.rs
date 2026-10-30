use chrono::{DateTime, Utc};
use num_bigint::BigInt;
use parquet::basic::{ConvertedType as C, LogicalType as L, TimeUnit, Type as P};
use parquet::file::statistics::Statistics;
use parquet::schema::types::ColumnDescriptor;
use std::cmp::Ordering;

#[derive(Debug, Clone)]
pub enum Kind {
    Int(u8, bool),
    Float(bool),
    Boolean,
    String,
    Decimal(i32, i32),
    Date,
    Time(u32),
    Timestamp(u32, bool),
    Binary,
    Uuid,
    Null,
    Unsupported(String),
}

fn digits(unit: &TimeUnit) -> u32 {
    match unit {
        TimeUnit::MILLIS => 3,
        TimeUnit::MICROS => 6,
        TimeUnit::NANOS => 9,
    }
}

impl Kind {
    pub fn of(col: &ColumnDescriptor) -> Self {
        if let Some(logical) = col.logical_type_ref() {
            return match logical {
                L::String | L::Enum | L::Json => Self::String,
                L::Integer(i) => Self::Int(i.bit_width as u8, i.is_signed),
                L::Decimal(d) => Self::Decimal(d.precision, d.scale),
                L::Date => Self::Date,
                L::Time(t) => Self::Time(digits(&t.unit)),
                L::Timestamp(t) => Self::Timestamp(digits(&t.unit), t.is_adjusted_to_u_t_c),
                L::Uuid => Self::Uuid,
                L::Unknown => Self::Null,
                other => Self::Unsupported(format!("{other:?}")),
            };
        }
        match col.converted_type() {
            C::UTF8 | C::ENUM | C::JSON => Self::String,
            C::DECIMAL => Self::Decimal(col.type_precision(), col.type_scale()),
            C::DATE => Self::Date,
            C::TIME_MILLIS => Self::Time(3),
            C::TIME_MICROS => Self::Time(6),
            C::TIMESTAMP_MILLIS => Self::Timestamp(3, true),
            C::TIMESTAMP_MICROS => Self::Timestamp(6, true),
            C::UINT_8 => Self::Int(8, false),
            C::UINT_16 => Self::Int(16, false),
            C::UINT_32 => Self::Int(32, false),
            C::UINT_64 => Self::Int(64, false),
            C::INT_8 => Self::Int(8, true),
            C::INT_16 => Self::Int(16, true),
            C::INT_32 => Self::Int(32, true),
            C::INT_64 => Self::Int(64, true),
            C::NONE => match col.physical_type() {
                P::BOOLEAN => Self::Boolean,
                P::INT32 => Self::Int(32, true),
                P::INT64 => Self::Int(64, true),
                P::FLOAT => Self::Float(true),
                P::DOUBLE => Self::Float(false),
                P::INT96 => Self::Unsupported("Int96 (legacy timestamp)".into()),
                _ => Self::Binary,
            },
            other => Self::Unsupported(format!("{other:?}")),
        }
    }

    pub fn label(&self, physical: P, length: i32) -> String {
        let unit = |d| match d {
            3 => "ms",
            6 => "us",
            _ => "ns",
        };
        match self {
            Self::Int(bits, signed) => format!("{}Int{bits}", if *signed { "" } else { "U" }),
            Self::Float(single) => if *single { "Float32" } else { "Float64" }.into(),
            Self::Boolean => "Boolean".into(),
            Self::String => "String".into(),
            Self::Decimal(p, s) => format!("Decimal({p},{s})"),
            Self::Date => "Date".into(),
            Self::Time(d) => format!("Time({})", unit(*d)),
            Self::Timestamp(d, utc) => format!(
                "Timestamp({}, {})",
                unit(*d),
                if *utc { "UTC" } else { "无时区" }
            ),
            Self::Uuid => "UUID".into(),
            Self::Null => "Null".into(),
            Self::Binary if physical == P::FIXED_LEN_BYTE_ARRAY => {
                format!("FixedSizeBinary({length})")
            }
            Self::Binary => "Binary".into(),
            Self::Unsupported(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Int(BigInt),
    Float(f64),
    Bytes(Vec<u8>),
    Boolean(bool),
}
impl Scalar {
    pub fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Some(a.cmp(b)),
            (Self::Float(a), Self::Float(b)) => {
                if a.is_nan() || b.is_nan() {
                    None
                } else {
                    Some(a.total_cmp(b))
                }
            }
            (Self::Bytes(a), Self::Bytes(b)) => Some(a.cmp(b)),
            (Self::Boolean(a), Self::Boolean(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }
}

pub fn bound(stats: &Statistics, kind: &Kind, min: bool) -> Option<Scalar> {
    let bytes = |value: &[u8]| {
        if matches!(kind, Kind::Decimal(..)) {
            Scalar::Int(BigInt::from_signed_bytes_be(value))
        } else {
            Scalar::Bytes(value.to_vec())
        }
    };
    match stats {
        Statistics::Boolean(s) => {
            (if min { s.min_opt() } else { s.max_opt() }).map(|v| Scalar::Boolean(*v))
        }
        Statistics::Int32(s) => (if min { s.min_opt() } else { s.max_opt() }).map(|v| {
            Scalar::Int(if matches!(kind, Kind::Int(_, false)) {
                BigInt::from(*v as u32)
            } else {
                BigInt::from(*v)
            })
        }),
        Statistics::Int64(s) => (if min { s.min_opt() } else { s.max_opt() }).map(|v| {
            Scalar::Int(if matches!(kind, Kind::Int(_, false)) {
                BigInt::from(*v as u64)
            } else {
                BigInt::from(*v)
            })
        }),
        Statistics::Float(s) => (if min { s.min_opt() } else { s.max_opt() })
            .filter(|v| !v.is_nan())
            .map(|v| Scalar::Float(*v as f64)),
        Statistics::Double(s) => (if min { s.min_opt() } else { s.max_opt() })
            .filter(|v| !v.is_nan())
            .map(|v| Scalar::Float(*v)),
        Statistics::ByteArray(s) => {
            (if min { s.min_opt() } else { s.max_opt() }).map(|v| bytes(v.data()))
        }
        Statistics::FixedLenByteArray(s) => {
            (if min { s.min_opt() } else { s.max_opt() }).map(|v| bytes(v.data()))
        }
        Statistics::Int96(_) => None,
    }
}

pub fn decimal(value: &BigInt, scale: i32) -> Option<String> {
    // Prevent a hostile scale from allocating gigabytes of padding.
    if scale.unsigned_abs() > 1000 {
        return None;
    }
    let text = value.to_string();
    let (sign, raw) = text
        .strip_prefix('-')
        .map_or(("", text.as_str()), |v| ("-", v));
    if scale <= 0 {
        return Some(format!("{text}{}", "0".repeat((-scale) as usize)));
    }
    let scale = scale as usize;
    if raw.len() <= scale {
        Some(format!("{sign}0.{}{raw}", "0".repeat(scale - raw.len())))
    } else {
        let at = raw.len() - scale;
        Some(format!("{sign}{}.{}", &raw[..at], &raw[at..]))
    }
}

pub fn timestamp(value: i64, digits: u32) -> Option<String> {
    let factor = 10i64.pow(digits);
    let date = DateTime::<Utc>::from_timestamp(
        value.div_euclid(factor),
        (value.rem_euclid(factor) * 10i64.pow(9 - digits)) as u32,
    )?;
    Some(format!(
        "{}.{:0width$}",
        date.format("%Y-%m-%d %H:%M:%S"),
        value.rem_euclid(factor),
        width = digits as usize
    ))
}

pub fn display(value: &Scalar, kind: &Kind, approximate: bool) -> Option<String> {
    match (value, kind) {
        (Scalar::Int(v), Kind::Decimal(_, scale)) => decimal(v, *scale),
        (Scalar::Int(v), Kind::Date) => {
            let days = v.to_string().parse::<i64>().ok()?;
            let date = DateTime::<Utc>::from_timestamp(days.checked_mul(86400)?, 0)?;
            Some(date.format("%Y-%m-%d").to_string())
        }
        (Scalar::Int(v), Kind::Timestamp(d, _)) => timestamp(v.to_string().parse().ok()?, *d),
        (Scalar::Int(v), Kind::Time(d)) => {
            let n = v.to_string().parse::<i64>().ok()?;
            let f = 10i64.pow(*d);
            if !(0..86400 * f).contains(&n) {
                return None;
            }
            Some(format!(
                "{:02}:{:02}:{:02}.{:0width$}",
                n / f / 3600,
                n / f / 60 % 60,
                n / f % 60,
                n % f,
                width = *d as usize
            ))
        }
        (Scalar::Int(v), Kind::Int(..)) => Some(v.to_string()),
        (Scalar::Float(v), Kind::Float(single)) => Some(if v.is_nan() {
            "NaN".into()
        } else if *v == f64::INFINITY {
            "Inf".into()
        } else if *v == f64::NEG_INFINITY {
            "-Inf".into()
        } else if *single {
            (*v as f32).to_string()
        } else {
            v.to_string()
        }),
        (Scalar::Boolean(v), Kind::Boolean) => Some(v.to_string()),
        (Scalar::Bytes(v), Kind::String) => {
            if approximate {
                Some(String::from_utf8_lossy(v).into())
            } else {
                String::from_utf8(v.clone()).ok()
            }
        }
        (Scalar::Bytes(v), Kind::Uuid) if v.len() == 16 => Some(format!(
            "{}-{}-{}-{}-{}",
            hex(&v[..4]),
            hex(&v[4..6]),
            hex(&v[6..8]),
            hex(&v[8..10]),
            hex(&v[10..])
        )),
        (Scalar::Bytes(v), Kind::Binary) => Some(format!(
            "<binary {} bytes> {}",
            v.len(),
            hex(&v[..v.len().min(16)])
        )),
        _ => None,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimal() {
        for (input, scale, expected) in [
            ("-1", 3, "-0.001"),
            (
                "90071992547409931234567890",
                4,
                "9007199254740993123456.7890",
            ),
            ("12", -2, "1200"),
            ("0", 2, "0.00"),
        ] {
            assert_eq!(decimal(&input.parse().unwrap(), scale).unwrap(), expected);
        }
        assert!(decimal(&1.into(), i32::MIN).is_none());
    }
    #[test]
    fn time_precision_and_negative_epoch() {
        assert_eq!(timestamp(-1, 9).unwrap(), "1969-12-31 23:59:59.999999999");
        assert_eq!(
            timestamp(1_704_164_645_123_456, 6).unwrap(),
            "2024-01-02 03:04:05.123456"
        );
        assert_eq!(
            display(&Scalar::Int(123456789i64.into()), &Kind::Time(9), false).unwrap(),
            "00:00:00.123456789"
        );
    }
    #[test]
    fn unsigned_and_float_precision() {
        assert_eq!(
            Scalar::Float(-0.0).compare(&Scalar::Float(0.0)),
            Some(Ordering::Less)
        );
        let s = Statistics::int64(Some(-1), Some(-1), None, Some(0), false);
        assert_eq!(
            display(
                &bound(&s, &Kind::Int(64, false), true).unwrap(),
                &Kind::Int(64, false),
                false
            )
            .unwrap(),
            "18446744073709551615"
        );
        assert_eq!(
            display(&Scalar::Float(0.1f32 as f64), &Kind::Float(true), false).unwrap(),
            "0.1"
        );
        assert!(
            !display(&Scalar::Float(1e-20), &Kind::Float(false), false)
                .unwrap()
                .contains('e')
        );
        assert_eq!(
            display(&Scalar::Float(f64::INFINITY), &Kind::Float(false), false).unwrap(),
            "Inf"
        );
        assert_eq!(
            display(&Scalar::Float(f64::NAN), &Kind::Float(false), false).unwrap(),
            "NaN"
        );
    }
    #[test]
    fn binary_is_not_automatically_uuid() {
        let bytes = Scalar::Bytes((0..16).collect());
        assert!(
            display(&bytes, &Kind::Binary, false)
                .unwrap()
                .starts_with("<binary 16 bytes>")
        );
        assert_eq!(
            display(&bytes, &Kind::Uuid, false).unwrap(),
            "00010203-0405-0607-0809-0a0b0c0d0e0f"
        );
    }
}
