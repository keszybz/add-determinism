/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, warn};
use nix::{errno, fcntl, sys, unistd};
use serde::{Serialize, Deserialize};
use std::env;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::str;

use crate::handlers;
use crate::options;

pub struct Controller {
    handlers: Vec<Box<dyn handlers::Processor>>,
    job_sockets: (OwnedFd, OwnedFd),
    result_sockets: (OwnedFd, OwnedFd),
    workers: Vec<process::Child>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Job {
    selected_handlers: u8,
    input_path: PathBuf,
}

impl Controller {
    fn build_worker_command(
        config: &options::Config,
        handlers: &[Box<dyn handlers::Processor>],
        job_fd: &RawFd,
        result_fd: &RawFd,
    ) -> Result<process::Command> {

        let mut cmd = process::Command::new(env::current_exe()?);

        cmd.arg("--job-socket").arg(job_fd.to_string());
        cmd.arg("--result-socket").arg(result_fd.to_string());
        if config.brp {
            cmd.arg("--brp");
        }
        if config.verbose {
            cmd.arg("-v");
        }
        cmd.arg("--handler")
            .arg(handlers
                 .iter()
                 .map(|x| x.name())
                 .collect::<Vec<&str>>()
                 .join(","));

        if let Some(val) = config.source_date_epoch {
            cmd.env("SOURCE_DATE_EPOCH", val.to_string());
        }

        Ok(cmd)
    }

    pub fn create(config: &Rc<options::Config>) -> Result<Self> {
        let handlers = handlers::make_handlers(config)?;

        let job_sockets = sys::socket::socketpair(
            sys::socket::AddressFamily::Unix,
            sys::socket::SockType::Datagram,
            None,
            sys::socket::SockFlag::empty(),
        )?;
        let result_sockets = sys::socket::socketpair(
            sys::socket::AddressFamily::Unix,
            sys::socket::SockType::Datagram,
            None,
            sys::socket::SockFlag::empty(),
        )?;

        fcntl::fcntl(job_sockets.1.as_raw_fd(), fcntl::F_SETFD(fcntl::FdFlag::FD_CLOEXEC))?;
        fcntl::fcntl(result_sockets.1.as_raw_fd(), fcntl::F_SETFD(fcntl::FdFlag::FD_CLOEXEC))?;
        fcntl::fcntl(result_sockets.1.as_raw_fd(), fcntl::F_SETFL(fcntl::OFlag::O_NONBLOCK))?;

        let mut cmd = Self::build_worker_command(
            config,
            &handlers,
            &job_sockets.0.as_raw_fd(),
            &result_sockets.0.as_raw_fd(),
        )?;

        let n = config.jobs.unwrap();
        let mut workers = vec![];

        for _ in 0..n {
            let child = cmd.spawn()?;
            workers.push(child);
        }

        Ok(Controller {
            handlers,
            job_sockets,
            result_sockets,
            workers,
        })
    }

    pub fn send_job(
        &self,
        selected_handlers: u8,
        input_path: &Path,
    ) -> Result<()> {

        assert!(selected_handlers > 0);

        let job = Job { selected_handlers, input_path: input_path.to_path_buf() };
        let buf = serde_cbor::ser::to_vec_packed(&job)?;

        debug!("Sending {:?}", &job);
        unistd::write(&self.job_sockets.1, &buf)?;

        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        debug!("Sending quit command to children…");
        for _ in &mut self.workers {
            unistd::write(&self.job_sockets.1, b"")?;
        }

        debug!("Closing control socket…");
        unistd::close(self.job_sockets.1.as_raw_fd())?;

        debug!("Waiting for children to exit…");
        for child in &mut self.workers {
            let status = child.wait()?;
            if status.success() {
                debug!("Child {} exited with success", child.id());
            } else if let Some(code) = status.code() {
                warn!("Child {} reported error {}", child.id(), code);
            } else if let Some(signal) = status.signal() {
                warn!("Child {} killed by signal {}", child.id(), signal);
            } else {
                panic!("Child was terminated by gremlins");
            }
        }
        debug!("Children are dead");
        Ok(())
    }

    fn read_results(&mut self, total: &mut handlers::Stats) -> Result<()> {
        debug!("Reading stats from children…");

        let mut count = 0;
        let mut buf = vec![0; 1024];

        loop {
            let n = match unistd::read(self.result_sockets.1.as_raw_fd(), &mut buf) {
                Err(e) => {
                    if e == errno::Errno::EAGAIN {
                        break;
                    } else {
                        return Err(anyhow!("read failed: {}", e));
                    }
                }
                Ok(n) => n,
            };

            if n == 0 {
                break;
            }

            let stats = serde_cbor::de::from_mut_slice(&mut buf[..n])?;
            debug!("Got result: {:?}", &stats);
            total.add(&stats);

            count += 1;
        }

        if count != self.workers.len() {
            warn!("Got {} results from {} workers. (??)", count, self.workers.len());
        }

        Ok(())
    }

    pub fn do_work(config: options::Config) -> Result<handlers::Stats> {
        let config = Rc::new(config);

        let mut control = Controller::create(&config)?;

        let mut inodes_seen = handlers::inodes_seen();
        let mut total = handlers::Stats::new();

        for input_path in &config.inputs {
            match handlers::process_file_or_dir(
                &control.handlers,
                &mut inodes_seen,
                input_path,
                Some(&|selected_handlers, input_path| control.send_job(selected_handlers, input_path)))
            {
                Err(err) => {
                    warn!("{}: failed to process: {}", input_path.display(), err);
                }
                Ok(stats) => {
                    total.add(&stats);
                }
            };
        }

        control.close()?;
        control.read_results(&mut total)?;
        Ok(total)
    }
}

fn process_file_with_selected_handlers(
    handlers: &[Box<dyn handlers::Processor>],
    selected_handlers: u8,
    input_path: &Path,
) -> Result<handlers::ProcessResult> {

    // check if selected_handlers doesn't have any unexpected entries
    if u8::BITS - selected_handlers.leading_zeros() > handlers.len().try_into().unwrap() {
        bail!("Bad handler mask 0x{selected_handlers:x}");
    }

    let mut entry_mod = handlers::ProcessResult::Noop;

    for (n_processor, processor) in handlers.iter().enumerate() {
        let cond = selected_handlers & (1 << n_processor) > 0;

        if cond {
            debug!("{}: running handler {}", input_path.display(), processor.name());
            let res = processor.process(input_path);
            entry_mod.extend_or_warn(input_path, res);
        }
    }

    Ok(entry_mod)
}

pub fn do_worker_work(config: options::Config) -> Result<()> {
    let job_socket = config.job_socket.unwrap();
    let job_socket = unsafe { UnixDatagram::from_raw_fd(job_socket) };

    let result_socket = config.result_socket.unwrap();
    let result_socket = unsafe { UnixDatagram::from_raw_fd(result_socket) };

    let config = Rc::new(config);
    let handlers = handlers::make_handlers(&config)?;
    let mut stats = handlers::Stats::new();

    let mut buf = vec![0; 4096]; // FIXME: use a better limit here?

    loop {
        let n = match job_socket.recv(buf.as_mut_slice()) {
            Err(e) => {
                bail!("recv failed: {e}");
            }
            Ok(n) => n
        };

        if n == 0 {
            break;
        }

        let job: Job = serde_cbor::de::from_mut_slice(&mut buf[..n])?;
        debug!("Got job {:?}", job);

        let res = process_file_with_selected_handlers(
            &handlers,
            job.selected_handlers,
            &job.input_path)?;
        stats.add_one(job.selected_handlers, res);
    }

    debug!("Worker {} wrapping up...", process::id());
    let buf = serde_cbor::ser::to_vec_packed(&stats)?;
    result_socket.send(&buf)?;

    debug!("Worker {} says bye!", process::id());
    Ok(())
}
