use crate::{
    enums::{Entry, EnumResult},
    meta::failure,
    sample::Result,
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct Store {
    items: Mutex<BTreeMap<String, (String, Arc<EnumResult>)>>,
    latest: Mutex<BTreeMap<(String, usize), String>>,
}
impl Store {
    pub fn put(&self, owner: &str, request: &str, result: &EnumResult) -> Result<EnumResult> {
        let mut result = result.clone();
        result.id = format!("{request}:{}", result.column.root);
        let summary = result.summary();
        self.latest
            .lock()
            .map_err(|e| failure("task_failed", e))?
            .insert((owner.into(), result.column.root), result.id.clone());
        let mut map = self.items.lock().map_err(|e| failure("task_failed", e))?;
        map.insert(result.id.clone(), (owner.into(), Arc::new(result)));
        while map.values().map(|(_, r)| r.size()).sum::<usize>() > 128 * 1024 * 1024 {
            if let Some(key) = map.keys().find(|k| **k != summary.id).cloned() {
                map.remove(&key);
            } else {
                break;
            }
        }
        Ok(summary)
    }
    pub fn get(&self, owner: &str, id: &str) -> Result<Arc<EnumResult>> {
        self.items
            .lock()
            .map_err(|e| failure("task_failed", e))?
            .get(id)
            .filter(|(o, _)| o == owner)
            .map(|(_, r)| r.clone())
            .ok_or_else(|| {
                failure(
                    "invalid_request",
                    "Enumeration result expired; analyze again",
                )
            })
    }
    pub fn latest(&self, owner: &str) -> Vec<Arc<EnumResult>> {
        let ids: Vec<_> = self
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|((o, _), _)| o == owner)
            .map(|(_, id)| id.clone())
            .collect();
        ids.iter()
            .filter_map(|id| self.get(owner, id).ok())
            .collect()
    }
    pub fn close(&self, owner: &str) {
        if let Ok(mut m) = self.latest.lock() {
            m.retain(|(o, _), _| o != owner);
        }
        if let Ok(mut m) = self.items.lock() {
            m.retain(|_, (o, _)| o != owner);
        }
    }
    pub fn clear(&self) {
        if let Ok(mut m) = self.latest.lock() {
            m.clear();
        }
        if let Ok(mut m) = self.items.lock() {
            m.clear();
        }
    }
}
#[derive(Serialize)]
pub struct Page {
    pub rows: Vec<Entry>,
    pub total: usize,
    pub offset: usize,
}
pub fn decimal_cmp(a: &str, b: &str) -> Ordering {
    fn special(v: &str) -> i8 {
        match v {
            "-Inf" => -1,
            "Inf" => 1,
            "NaN" => 2,
            _ => 0,
        }
    }
    let rank = special(a).cmp(&special(b));
    if rank != Ordering::Equal {
        return rank;
    }
    let (an, bn) = (a.starts_with('-'), b.starts_with('-'));
    if an != bn {
        return if an {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    let a = a.trim_start_matches('-');
    let b = b.trim_start_matches('-');
    let (ai, af) = a.split_once('.').unwrap_or((a, ""));
    let (bi, bf) = b.split_once('.').unwrap_or((b, ""));
    let ai = ai.trim_start_matches('0');
    let bi = bi.trim_start_matches('0');
    let mut cmp = ai.len().cmp(&bi.len()).then_with(|| ai.cmp(bi));
    if cmp == Ordering::Equal {
        cmp = af.trim_end_matches('0').cmp(bf.trim_end_matches('0'));
    }
    if an { cmp.reverse() } else { cmp }
}
pub fn page(
    result: &EnumResult,
    offset: usize,
    limit: usize,
    search: &str,
    sort: &str,
    descending: bool,
) -> Result<Page> {
    if !(1..=500).contains(&limit) || !matches!(sort, "value" | "count") {
        return Err(failure("invalid_request", "limit 1..500; sort value/count"));
    }
    let search = search.to_lowercase();
    let mut values: Vec<_> = result
        .values
        .iter()
        .filter(|v| {
            v.value
                .as_deref()
                .map_or("<NULL>", |s| if s.is_empty() { "<空字符串>" } else { s })
                .to_lowercase()
                .contains(&search)
        })
        .collect();
    values.sort_by(|a, b| {
        let by_value = || match (&a.value, &b.value) {
            (None, None) => Ordering::Equal,
            (None, _) => Ordering::Greater,
            (_, None) => Ordering::Less,
            (Some(a), Some(b)) => {
                if result.column.numeric {
                    decimal_cmp(a, b)
                } else if result.column.data_type.contains("Binary") && !result.column.uuid {
                    a.split_once("> ")
                        .map(|(_, v)| v)
                        .cmp(&b.split_once("> ").map(|(_, v)| v))
                } else {
                    a.cmp(b)
                }
            }
        };
        let order = if sort == "count" {
            a.count
                .parse::<u64>()
                .unwrap_or(0)
                .cmp(&b.count.parse().unwrap_or(0))
                .then_with(by_value)
        } else {
            by_value()
        };
        if descending { order.reverse() } else { order }
    });
    Ok(Page {
        total: values.len(),
        rows: values
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect(),
        offset,
    })
}
