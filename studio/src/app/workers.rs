//! Background threads for everything that touches the disk or takes long on the CPU, so
//! the interface never waits for them. Two queues: what the screen is waiting for goes
//! ahead of opening the rest of a large import.

use alexander_studio::export::{self, Format, Picture, Size};
use alexander_studio::library::Id;
use alexander_studio::load::{self, Opened, Pixels};
use alexander_studio::provenance::Record;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};

pub enum Job {
    Collect(Vec<PathBuf>),
    Open { id: Id, path: PathBuf },
    Full { id: Id, path: PathBuf, long: u32 },
    Encode { pic: Box<Picture>, size: Size, original_long: u32, format: Format, record: Box<Record>, path: PathBuf },
}

pub enum Done {
    Collected(Vec<PathBuf>),
    Opened { id: Id, result: Result<Box<Opened>, String> },
    Full { id: Id, long: u32, result: Result<Pixels, String> },
    Encoded { path: PathBuf, result: Result<(), String> },
}

#[derive(Default)]
struct Queues {
    urgent: VecDeque<Job>,
    later: VecDeque<Job>,
    stop: bool,
}

pub struct Workers {
    shared: Arc<(Mutex<Queues>, Condvar)>,
    pub rx: Receiver<Done>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Workers {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Workers {
        let shared = Arc::new((Mutex::new(Queues::default()), Condvar::new()));
        let (tx, rx) = channel();
        let wake = Arc::new(wake);
        // decoding a large photo holds a few hundred MB, so not one thread per core
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(2, 6);
        let threads = (0..n)
            .map(|i| {
                let (shared, tx, wake) = (shared.clone(), tx.clone(), wake.clone());
                std::thread::Builder::new().name(format!("worker-{i}")).spawn(move || run(shared, tx, wake)).expect("thread")
            })
            .collect();
        Workers { shared, rx, threads }
    }

    pub fn send(&self, job: Job) {
        let (m, cv) = &*self.shared;
        let mut q = m.lock().unwrap();
        match job {
            Job::Open { .. } => q.later.push_back(job),
            Job::Full { .. } => q.urgent.push_front(job),
            _ => q.urgent.push_back(job),
        }
        cv.notify_one();
    }

    /// Jobs waiting for a thread.
    pub fn queued(&self) -> usize {
        let q = self.shared.0.lock().unwrap();
        q.urgent.len() + q.later.len()
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        {
            let (m, cv) = &*self.shared;
            let mut q = m.lock().unwrap();
            q.stop = true;
            q.urgent.clear();
            q.later.clear();
            cv.notify_all();
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn run(shared: Arc<(Mutex<Queues>, Condvar)>, tx: Sender<Done>, wake: Arc<dyn Fn() + Send + Sync>) {
    loop {
        let job = {
            let (m, cv) = &*shared;
            let mut q = m.lock().unwrap();
            loop {
                if q.stop {
                    return;
                }
                if let Some(j) = q.urgent.pop_front().or_else(|| q.later.pop_front()) {
                    break j;
                }
                q = cv.wait(q).unwrap();
            }
        };
        let done = match job {
            Job::Collect(paths) => Done::Collected(load::collect(&paths)),
            Job::Open { id, path } => Done::Opened { id, result: load::open(&path).map(Box::new) },
            Job::Full { id, path, long } => Done::Full { id, long, result: load::full(&path, long) },
            Job::Encode { pic, size, original_long, format, record, path } => {
                let result = export::write(&pic, size, original_long, format, &record, &path);
                Done::Encoded { path, result }
            }
        };
        if tx.send(done).is_err() {
            return;
        }
        wake();
    }
}
