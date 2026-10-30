use crate::{
    enum_key,
    meta::failure,
    sample::{Result, check},
};
use arrow::{array::*, datatypes::*};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
pub const COLUMN_BYTES: usize = 16 * 1024 * 1024;
pub const TASK_BYTES: usize = 128 * 1024 * 1024;
#[derive(Default)]
pub struct Budget(pub AtomicUsize);
pub struct Counter {
    numbers: HashMap<u64, usize>,
    bytes: HashMap<Vec<u8>, usize>,
    pub values: Vec<(u64, String)>,
    pub nulls: u64,
    pub scanned: u64,
    pub status: &'static str,
    held: usize,
    budget: Arc<Budget>,
    cap: usize,
    pub groups: usize,
}
impl Counter {
    pub fn new(cap: usize, budget: Arc<Budget>) -> Self {
        Self {
            numbers: HashMap::new(),
            bytes: HashMap::new(),
            values: Vec::new(),
            nulls: 0,
            scanned: 0,
            status: "running",
            held: 0,
            budget,
            cap,
            groups: 0,
        }
    }
    pub fn stop(&mut self, status: &'static str) {
        self.status = status;
        self.numbers = HashMap::new();
        self.bytes = HashMap::new();
        self.values = Vec::new();
        self.budget.0.fetch_sub(self.held, Ordering::Relaxed);
        self.held = 0;
    }
    fn reserve(&mut self, size: usize) -> bool {
        if self.held.saturating_add(size) > COLUMN_BYTES {
            self.stop("resource");
            return false;
        }
        let reserved = self
            .budget
            .0
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(size).filter(|v| *v <= TASK_BYTES)
            });
        if reserved.is_err() {
            self.stop("resource");
            return false;
        }
        self.held += size;
        true
    }
    fn number(&mut self, key: u64, n: u64, display: impl FnOnce() -> Result<String>) -> Result<()> {
        if let Some(&idx) = self.numbers.get(&key) {
            self.values[idx].0 += n;
            return Ok(());
        }
        if self.values.len() >= self.cap {
            self.stop("high");
            return Ok(());
        }
        let value = display()?;
        if !self.reserve(128 + value.len()) {
            return Ok(());
        }
        self.numbers.insert(key, self.values.len());
        self.values.push((n, value));
        Ok(())
    }
    fn binary(
        &mut self,
        key: &[u8],
        n: u64,
        display: impl FnOnce() -> Result<String>,
    ) -> Result<()> {
        if let Some(&idx) = self.bytes.get(key) {
            self.values[idx].0 += n;
            return Ok(());
        }
        if self.values.len() >= self.cap {
            self.stop("high");
            return Ok(());
        }
        if key.len() > COLUMN_BYTES / 2 {
            self.stop("resource");
            return Ok(());
        }
        let value = display()?;
        if !self.reserve(128 + key.len() + value.len()) {
            return Ok(());
        }
        self.bytes.insert(key.to_vec(), self.values.len());
        self.values.push((n, value));
        Ok(())
    }
    fn one(&mut self, a: &dyn Array, r: usize, n: u64, uuid: bool) -> Result<()> {
        self.scanned += n;
        if a.is_null(r) {
            self.nulls += n;
            return Ok(());
        }
        let (a, r) = enum_key::dictionary(a, r)?;
        if a.is_null(r) {
            self.nulls += n;
            return Ok(());
        }
        if let Some(bytes) = enum_key::bytes(a, r) {
            return self.binary(bytes, n, || enum_key::display(a, r, uuid));
        }
        let mut key = Vec::new();
        if let Err(e) = enum_key::nested(a, r, &mut key) {
            if e.code == "resource_limit" {
                self.stop("resource");
                return Ok(());
            }
            return Err(e);
        }
        self.binary(&key, n, || enum_key::display(a, r, uuid))
    }
    pub fn add(&mut self, a: &dyn Array, uuid: bool, cancel: &AtomicBool) -> Result<()> {
        if self.status != "running" {
            return Ok(());
        }
        // Primitive counters use native bits and format each distinct value only once.
        macro_rules! primitive {
            ($t:ty,$key:expr) => {{
                let values = a.as_any().downcast_ref::<PrimitiveArray<$t>>().unwrap();
                for (r, v) in values.iter().enumerate() {
                    if r % 1024 == 0 {
                        check(cancel)?;
                    }
                    self.scanned += 1;
                    if let Some(v) = v {
                        self.number(($key)(v), 1, || enum_key::display(a, r, uuid))?;
                    } else {
                        self.nulls += 1;
                    }
                    if self.status != "running" {
                        break;
                    }
                }
                return Ok(());
            }};
        }
        macro_rules! dict {
            ($t:ty) => {{
                let d = a.as_any().downcast_ref::<DictionaryArray<$t>>().unwrap();
                if d.values().len() <= 1_000_000 {
                    let mut frequencies = vec![0u64; d.values().len()];
                    for (r, k) in d.keys().iter().enumerate() {
                        if r % 1024 == 0 {
                            check(cancel)?;
                        }
                        if let Some(k) = k {
                            frequencies[k as usize] += 1;
                        } else {
                            self.scanned += 1;
                            self.nulls += 1;
                        }
                    }
                    for (i, n) in frequencies.into_iter().enumerate() {
                        if i % 1024 == 0 {
                            check(cancel)?;
                        }
                        if n > 0 {
                            self.one(d.values().as_ref(), i, n, uuid)?;
                            if self.status != "running" {
                                break;
                            }
                        }
                    }
                    return Ok(());
                }
            }};
        }
        match a.data_type() {
            DataType::Int8 => primitive!(Int8Type, |v: i8| v as u64),
            DataType::Int16 => primitive!(Int16Type, |v: i16| v as u64),
            DataType::Int32 => primitive!(Int32Type, |v: i32| v as u64),
            DataType::Int64 => primitive!(Int64Type, |v: i64| v as u64),
            DataType::UInt8 => primitive!(UInt8Type, |v: u8| v as u64),
            DataType::UInt16 => primitive!(UInt16Type, |v: u16| v as u64),
            DataType::UInt32 => primitive!(UInt32Type, |v: u32| v as u64),
            DataType::UInt64 => primitive!(UInt64Type, |v: u64| v),
            DataType::Date32 => primitive!(Date32Type, |v: i32| v as u64),
            DataType::Date64 => primitive!(Date64Type, |v: i64| v as u64),
            DataType::Float32 => primitive!(Float32Type, |v: f32| if v.is_nan() {
                f32::NAN.to_bits() as u64
            } else {
                v.to_bits() as u64
            }),
            DataType::Float64 => primitive!(Float64Type, |v: f64| if v.is_nan() {
                f64::NAN.to_bits()
            } else {
                v.to_bits()
            }),
            DataType::Dictionary(k, _) => match k.as_ref() {
                DataType::Int8 => dict!(Int8Type),
                DataType::Int16 => dict!(Int16Type),
                DataType::Int32 => dict!(Int32Type),
                DataType::Int64 => dict!(Int64Type),
                DataType::UInt8 => dict!(UInt8Type),
                DataType::UInt16 => dict!(UInt16Type),
                DataType::UInt32 => dict!(UInt32Type),
                DataType::UInt64 => dict!(UInt64Type),
                _ => return Err(failure("parse_failed", "Unsupported dictionary key")),
            },
            _ => {}
        }
        for r in 0..a.len() {
            if r % 256 == 0 {
                check(cancel)?;
            }
            self.one(a, r, 1, uuid)?;
            if self.status != "running" {
                break;
            }
        }
        Ok(())
    }
}
impl Drop for Counter {
    fn drop(&mut self) {
        self.budget.0.fetch_sub(self.held, Ordering::Relaxed);
    }
}
