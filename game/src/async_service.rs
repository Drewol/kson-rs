use di::{inject, injectable};

use crate::{config::GameConfig, worker_service::WorkerService};

use std::sync::{Arc, Mutex};

#[derive(Clone)]

pub struct AsyncService {
    jobs: Arc<Mutex<Vec<poll_promise::Promise<()>>>>,
}

impl WorkerService for AsyncService {
    fn update(&mut self) {
        self.jobs.lock().unwrap().retain(|x| x.poll().is_pending())
    }
}

#[injectable]
impl AsyncService {
    #[inject]
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(vec![])),
        }
    }

    pub fn save_config(&self) {
        self.run(async { GameConfig::get().save() })
    }

    pub fn run(&self, job: impl std::future::Future<Output = ()> + Send + 'static) {
        self.jobs
            .lock()
            .unwrap()
            .push(poll_promise::Promise::spawn_async(job))
    }
}

impl Default for AsyncService {
    fn default() -> Self {
        Self::new()
    }
}
