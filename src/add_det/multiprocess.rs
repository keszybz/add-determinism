/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::debug;
use std::thread;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use crossbeam_channel::{Sender,Receiver};

use crate::config;
use crate::handlers;

pub struct Controller {
    config: Arc<config::Config>,
    handlers: Arc<Vec<Box<dyn handlers::Processor + Send + Sync>>>,

    workers: Option<Vec<thread::JoinHandle<()>>>,

    job_tx: Sender<Option<Job>>,
    answer_rx: Receiver<handlers::Stats>,
}

#[derive(Debug)]
pub struct Job {
    selected_handlers: u8,
    input_path: PathBuf,
}

impl Controller {
    pub fn create(config: &Arc<config::Config>) -> Result<Self> {
        let handlers = Arc::new(handlers::make_handlers(config)?);

        let n = config.jobs.unwrap() as usize;
        assert!(n > 0);

        // Make the queue a little larger then the pool of workers
        // to let a few jobs accumulate if the sender gets ahead.
        let job_queue = crossbeam_channel::bounded(n*3/2 + 5);
        let answer_queue = crossbeam_channel::unbounded();

        let workers: Vec<_> = (0..n).map(|_| {
            let handlers = handlers.clone();
            let job_rx = job_queue.1.clone();
            let answer_tx = answer_queue.0.clone();

            thread::spawn(move || {
                do_worker_work(
                    handlers,
                    job_rx,
                    answer_tx);
            })
        }).collect();

        Ok(Controller {
            config: config.clone(),
            handlers,

            workers: Some(workers),

            job_tx: job_queue.0,
            answer_rx: answer_queue.1,
            // The other parts of job_queue and answer_queue will be
            // dropped when this function returns.
        })
    }

    pub fn send_job(
        &self,
        selected_handlers: u8,
        input_path: &Path,
    ) -> Result<()> {

        assert!(selected_handlers > 0);

        let job = Job { selected_handlers, input_path: input_path.to_path_buf() };
        debug!("Sending {:?}", &job);
        self.job_tx.send(Some(job))?;

        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        let workers = self.workers.take().unwrap();

        debug!("Sending quit command to threads…");
        for _ in &workers {
            self.job_tx.send(None)?;
        }

        debug!("Waiting for threads to exit…");

        for child in workers.into_iter() {
            if let Err(e) = child.join() {
                bail!("Thread failed: {e:?}");
            }
        }

        debug!("Threads are dead");
        Ok(())
    }

    fn read_results(&mut self, total: &mut handlers::Stats) -> Result<()> {
        debug!("Reading stats from children…");

        for _ in 0..self.config.jobs.unwrap() {
            let stats = self.answer_rx.recv()?;
            debug!("Got result: {:?}", &stats);
            total.add(&stats);
        }

        Ok(())
    }

    pub fn do_work(config: &Arc<config::Config>) -> Result<handlers::Stats> {
        let mut control = Controller::create(config)?;

        let mut inodes_seen = handlers::inodes_seen();
        let mut total = handlers::Stats::new();

        for input_path in &config.inputs {
            let stats = handlers::process_file_or_dir(
                &control.handlers,
                &mut inodes_seen,
                input_path,
                Some(&|selected_handlers, input_path| control.send_job(selected_handlers, input_path)));
            total.add(&stats);
        }

        control.close()?;
        control.read_results(&mut total)?;
        Ok(total)
    }
}

fn process_file_with_selected_handlers(
    handlers: &[Box<dyn handlers::Processor + Send + Sync>],
    selected_handlers: u8,
    input_path: &Path,
) -> Result<handlers::ProcessResult> {

    // check if selected_handlers doesn't have any unexpected entries
    if u8::BITS - selected_handlers.leading_zeros() > handlers.len() as u32 {
        bail!("Bad handler mask 0x{selected_handlers:x}");
    }

    let mut entry_mod = handlers::ProcessResult::Noop;

    for (n_processor, processor) in handlers.iter().enumerate() {
        let cond = selected_handlers & (1 << n_processor) > 0;

        if cond {
            debug!("{}: running handler {}", input_path.display(), processor.name());
            let res = processor.process(input_path);
            entry_mod.extend_and_warn(input_path, res);
        }
    }

    Ok(entry_mod)
}

pub fn do_worker_work(
    handlers: Arc<Vec<Box<dyn handlers::Processor + Send + Sync>>>,
    job_rx: Receiver<Option<Job>>,
    answer_tx: Sender<handlers::Stats>,
) {
    let mut stats = handlers::Stats::new();

    while let Some(job) = job_rx.recv().unwrap() {
        debug!("Got job {job:?}");
        assert!(job.selected_handlers > 0);

        match process_file_with_selected_handlers(
            &handlers,
            job.selected_handlers,
            &job.input_path)
        {
            Ok(res) => stats.add_one(res),
            Err(_) => { stats.errors += 1 }
        }
    }

    debug!("Wrapping up...");
    answer_tx.send(stats).unwrap();
    debug!("Worker says bye!");
}
