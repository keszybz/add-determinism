/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, info, warn, error};
use nix::{errno, fcntl, sys, unistd};
use serde::{Serialize, Deserialize};
use std::{cmp, env, process, str};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::config;
use crate::handlers;

pub struct Controller {
    handlers: Vec<Box<dyn handlers::Processor>>,
    job_sockets: Option<(OwnedFd, OwnedFd)>,
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
        config: &config::Config,
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
        if config.check {
            cmd.arg("--check");
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

    fn set_socket_size(fd: &OwnedFd, n: usize) -> Result<usize> {
        /* Let's assume our messages are not larger than 4096 bytes
         * and that we cannot push more than 256 messages into the queue. */
        let newsize = cmp::min(n, 256) * 4096;

        let mut sndbuf = sys::socket::getsockopt(fd, sys::socket::sockopt::SndBuf)?;
        debug!("Initial socket buffer size: {}", sndbuf);

        if newsize > sndbuf {
            sys::socket::setsockopt(fd, sys::socket::sockopt::SndBuf, &newsize)?;

            sndbuf = sys::socket::getsockopt(fd, sys::socket::sockopt::SndBuf)?;
            debug!("Tried to set socket buffer size to {}, got {}", newsize, sndbuf);

            if newsize > sndbuf {
                if let Err(err) = sys::socket::setsockopt(
                    fd,
                    sys::socket::sockopt::SndBufForce,
                    &newsize,
                ) {
                    debug!("Cannot set buffer size to {}: {}", newsize, err);
                } else {
                    sndbuf = sys::socket::getsockopt(fd, sys::socket::sockopt::SndBuf)?;
                    debug!("Tried to force socket buffer size to {}, got {}", newsize, sndbuf);
                }
            }
        }

        /* The kernel may set the size higher than we requested, but
         * don't go above our initial goal. */
        Ok(cmp::min(newsize, sndbuf) / 4096)
    }

    pub fn create(config: &Rc<config::Config>) -> Result<Self> {
        let handlers = handlers::make_handlers(config)?;

        let mut n = config.jobs.unwrap() as usize;
        assert!(n > 0);

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

        let n2 = Self::set_socket_size(&job_sockets.0, n)?;
        if n2 < n {
            info!("Limiting number of workers to {}", n2);
            n = n2;
        }

        Self::set_socket_size(&result_sockets.0, n)?;

        let mut cmd = Self::build_worker_command(
            config,
            &handlers,
            &job_sockets.0.as_raw_fd(),
            &result_sockets.0.as_raw_fd(),
        )?;

        let mut workers = vec![];
        for _ in 0..n {
            let child = cmd.spawn()?;
            workers.push(child);
        }

        Ok(Controller {
            handlers,
            job_sockets: Some(job_sockets),
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

        debug!("Sending {:?} ({} bytes)", &job, buf.len());
        unistd::write(&self.job_sockets.as_ref().unwrap().1, &buf)?;

        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        debug!("Sending quit command to children…");
        for _ in &mut self.workers {
            unistd::write(&self.job_sockets.as_ref().unwrap().1, b"")?;
        }

        debug!("Closing control socket…");
        self.job_sockets.take();

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
                        bail!("read failed: {e}");
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
            error!("Got {} results from {} workers. (??)", count, self.workers.len());
            total.errors += 1;
        }

        Ok(())
    }

    pub fn do_work(config: &Rc<config::Config>) -> Result<handlers::Stats> {
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
            entry_mod.extend_and_warn(input_path, res);
        }
    }

    Ok(entry_mod)
}

pub fn do_worker_work(config: &Rc<config::Config>) -> Result<()> {
    let job_socket = config.job_socket.unwrap();
    let job_socket = unsafe { UnixDatagram::from_raw_fd(job_socket) };

    let result_socket = config.result_socket.unwrap();
    let result_socket = unsafe { UnixDatagram::from_raw_fd(result_socket) };

    let handlers = handlers::make_handlers(config)?;
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

        assert!(job.selected_handlers > 0);

        let res = process_file_with_selected_handlers(
            &handlers,
            job.selected_handlers,
            &job.input_path)?;
        stats.add_one(res);
    }

    debug!("Wrapping up...");
    let buf = serde_cbor::ser::to_vec_packed(&stats)?;
    result_socket.send(&buf)?;

    debug!("Worker says bye!");
    Ok(())
}
