use crate::{files::OpenError, meta::failure};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
pub struct Tasks {
    active: Mutex<Option<Arc<Job>>>,
    finished: Condvar,
    app: Mutex<Option<tauri::AppHandle>>,
}
pub struct Job {
    pub owner: String,
    pub request: String,
    pub cancel: AtomicBool,
}
pub struct Guard {
    pub job: Arc<Job>,
    tasks: Arc<Tasks>,
}
impl Tasks {
    pub fn attach(&self, app: tauri::AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }
    fn emit(&self, job: Option<&Job>) {
        use tauri::Emitter;
        if let Some(app) = self.app.lock().unwrap().as_ref() {
            let _ = app.emit(
                "task_state",
                job.map(|j| serde_json::json!({"owner":j.owner,"request":j.request})),
            );
        }
    }
    pub fn status(&self) -> Option<serde_json::Value> {
        self.active
            .lock()
            .unwrap()
            .as_ref()
            .map(|j| serde_json::json!({"owner":j.owner,"request":j.request}))
    }
    pub fn cancel_and_wait(&self, owner: &str) -> Result<(), OpenError> {
        let mut a = self.active.lock().map_err(|e| failure("task_failed", e))?;
        if let Some(j) = a.as_ref().filter(|j| j.owner == owner) {
            j.cancel.store(true, Ordering::Relaxed);
            let job = j.clone();
            while a.as_ref().is_some_and(|j| Arc::ptr_eq(j, &job)) {
                a = self
                    .finished
                    .wait(a)
                    .map_err(|e| failure("task_failed", e))?;
            }
        }
        Ok(())
    }

    pub fn begin(self: &Arc<Self>, owner: String, request: String) -> Result<Guard, OpenError> {
        let mut active = self.active.lock().map_err(|e| failure("task_failed", e))?;
        if active.is_some() {
            return Err(failure(
                "task_busy",
                "Another task is finishing; retry shortly",
            ));
        }
        let job = Arc::new(Job {
            owner,
            request,
            cancel: AtomicBool::new(false),
        });
        *active = Some(job.clone());
        self.emit(Some(&job));
        Ok(Guard {
            job,
            tasks: self.clone(),
        })
    }
    pub fn cancel(&self, owner: &str, request: Option<&str>) -> Result<(), OpenError> {
        if let Some(job) = self
            .active
            .lock()
            .map_err(|e| failure("task_failed", e))?
            .as_ref()
            && job.owner == owner
            && request.is_none_or(|r| r == job.request)
        {
            job.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.tasks.active.lock()
            && active.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.job))
        {
            *active = None;
            self.tasks.emit(None);
            self.tasks.finished.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_scoping_busy_and_panic_recovery() {
        let tasks = Arc::new(Tasks::default());
        let guard = tasks.begin("a".into(), "r1".into()).unwrap();
        assert!(tasks.begin("b".into(), "r2".into()).is_err());
        tasks.cancel("b", None).unwrap();
        assert!(!guard.job.cancel.load(Ordering::Relaxed));
        tasks.cancel("a", Some("stale")).unwrap();
        assert!(!guard.job.cancel.load(Ordering::Relaxed));
        tasks.cancel("a", Some("r1")).unwrap();
        assert!(guard.job.cancel.load(Ordering::Relaxed));
        drop(guard);
        let cloned = tasks.clone();
        let _ = std::panic::catch_unwind(move || {
            let _guard = cloned.begin("a".into(), "r3".into()).unwrap();
            panic!("test task panic");
        });
        assert!(tasks.begin("a".into(), "r4".into()).is_ok());
    }
}
