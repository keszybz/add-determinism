/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Result};
use log::{debug, warn};
use nix::{fcntl, sys, unistd};
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
    sockets: (OwnedFd, OwnedFd),
    workers: Vec<process::Child>,
}

impl Controller {
    fn build_worker_command(
        config: &options::Config,
        handlers: &[Box<dyn handlers::Processor>],
        fd: &RawFd,
    ) -> Result<process::Command> {

        let mut cmd = process::Command::new(env::current_exe()?);

        cmd.arg("--socket").arg(fd.to_string());
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

        let sockets = sys::socket::socketpair(
            sys::socket::AddressFamily::Unix,
            sys::socket::SockType::Datagram,
            None,
            sys::socket::SockFlag::empty(),
        )?;

        fcntl::fcntl(sockets.1.as_raw_fd(), fcntl::F_SETFD(fcntl::FdFlag::FD_CLOEXEC))?;

        let mut cmd = Self::build_worker_command(config, &handlers, &sockets.0.as_raw_fd())?;
        // dbg!(&cmd);

        let n = config.jobs.unwrap();
        let mut workers = vec![];

        for _ in 0..n {
            let child = cmd.spawn()?;
            workers.push(child);
        }

        Ok(Controller {
            handlers,
            sockets,
            workers,
        })
    }

    pub fn send_path(
        &self,
        selected_handlers: u8,
        input_path: &Path,
    ) -> Result<()> {

        assert!(selected_handlers > 0);

        let arg = input_path.to_str().unwrap().as_bytes();
        let mut buf = vec![selected_handlers];
        buf.extend(arg);

        debug!("Sending {} (handlers=0x{:x})", input_path.display(), selected_handlers);
        unistd::write(&self.sockets.1, &buf)?;

        Ok(())
    }

    pub fn close(&mut self) -> Result<()> {
        debug!("Sending quit command to children…");
        for _ in &mut self.workers {
            unistd::write(&self.sockets.1, b"")?;
        }

        debug!("Closing control socket…");
        unistd::close(self.sockets.1.as_raw_fd())?;

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

    pub fn do_work(config: options::Config) -> Result<u64> {
        let config = Rc::new(config);

        let mut control = Controller::create(&config)?;

        let mut inodes_seen = handlers::inodes_seen();
        let mut n_paths = 0;

        for input_path in &config.inputs {
            match handlers::process_file_or_dir(
                &control.handlers,
                &mut inodes_seen,
                input_path,
                Some(&|selected_handlers, input_path| control.send_path(selected_handlers, input_path)))
            {
                Err(err) => {
                    warn!("{}: failed to process: {}", input_path.display(), err);
                }
                Ok(num) => {
                    n_paths += num;
                }
            };
        }

        control.close()?;
        Ok(n_paths)
    }
}

pub fn process_file_with_selected_handlers(
    handlers: &[Box<dyn handlers::Processor>],
    selected_handlers: u8,
    input_path: &Path,
) -> Result<bool> {

    let mut entry_mod = false;

    // check if selected_handlers doesn't have any unexpected entries
    if u8::BITS - selected_handlers.leading_zeros() > handlers.len().try_into().unwrap() {
        bail!("Bad handler mask 0x{selected_handlers:x}");
    }

    for (n_processor, processor) in handlers.iter().enumerate() {
        let cond = selected_handlers & (1 << n_processor) > 0;

        if cond {
            debug!("{}: running handler {}", input_path.display(), processor.name());

            match processor.process(input_path) {
                Err(err) => {
                    warn!("{}: failed to process: {}", input_path.display(), err);
                }
                Ok(false) => {}
                Ok(true) => {
                    entry_mod = true;
                }
            }
        }
    }

    Ok(entry_mod)
}

pub fn do_worker_work(config: options::Config) -> Result<()> {
    let socket = config.socket.unwrap();
    let socket = unsafe { UnixDatagram::from_raw_fd(socket) };

    let config = Rc::new(config);
    let handlers = handlers::make_handlers(&config)?;

    let mut buf = vec![0; 4096]; // FIXME: use a better limit here?

    loop {
        let n = match socket.recv(buf.as_mut_slice()) {
            Err(e) => {
                bail!("recv failed: {e}");
            }
            Ok(n) => n
        };

        if n == 0 {
            debug!("Worker {} says bye!", process::id());
            return Ok(());
        }

        let selected_handlers = buf[0];
        let input = str::from_utf8(&buf[1..n])?;

        if input.is_empty() {
            panic!("Empty input path");
        }

        debug!("Will process {input:?} (selected_handlers={selected_handlers})");
        let input_path = PathBuf::from(input);

        if let Err(e) =
            process_file_with_selected_handlers(&handlers, selected_handlers, &input_path)
        {
            warn!("{}: failed to process: {}", input_path.display(), e);
        }
    }
}
