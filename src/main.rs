/* SPDX-License-Identifier: GPL-3.0-or-later */

mod handlers;
mod options;
mod simplelog;

use anyhow::{Result, anyhow};
use log::{debug, info, warn};
use nix::{fcntl, sys, unistd};
use std::env;
use std::os::fd::{OwnedFd, RawFd, AsRawFd, FromRawFd};
use std::os::unix::net::UnixDatagram;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;
use std::str;

fn brp_check(config: &options::Config) -> Result<()> {
    // env::current_exe() does readlink("/proc/self/exe"), which returns
    // the target binary, so we cannot use that.

    let arg0 = env::args().next().unwrap();

    let brp = config.brp || Path::new(&arg0)
        .file_name()
        .ok_or(anyhow!("Exe path doesn't have a file name?"))?
        .to_str()
        .ok_or(anyhow!("Exe file name is not valid unicode"))?
        .starts_with("brp-");

    debug!("Running as {}… (brp={})", arg0, if brp { "true" } else { "false" });

    if brp {
        let build_root = env::var("RPM_BUILD_ROOT")
            .map_err(|_| anyhow!("RPM_BUILD_ROOT variable is not defined"))?;

        if build_root.is_empty() {
            return Err(anyhow!("Empty RPM_BUILD_ROOT is not allowed"));
        }

        let build_root_path = Path::new(&build_root).canonicalize()
            .map_err(|e| anyhow!("Cannot canonicalize RPM_BUILD_ROOT={:?}: {}", build_root, e))?;

        if build_root_path == Path::new("/") {
            return Err(anyhow!("RPM_BUILD_ROOT={:?} is not allowed", build_root));
        }

        for arg in &config.args {
            if !arg.starts_with(&build_root_path) {
                return Err(anyhow!("Path {:?} is outside of $RPM_BUILD_ROOT", arg));
            }
        }
    }

    Ok(())
}

pub struct Controller {
    sockets: (OwnedFd, OwnedFd),
    workers: Vec<process::Child>,
}

impl Controller {
    fn build_worker_command(config: &options::Config, fd: &RawFd) -> Result<process::Command> {
        let mut cmd = process::Command::new(env::current_exe()?);
        cmd.arg("--socket").arg(fd.to_string());
        if config.brp {
            cmd.arg("--brp");
        }
        if config.verbose {
            cmd.arg("--verbose");
        }
        if let Some(val) = config.source_date_epoch {
            cmd.env("SOURCE_DATE_EPOCH", val.to_string());
        }
        Ok(cmd)
    }

    pub fn spawn(config: &options::Config) -> Result<Self> {
        let n = config.jobs.unwrap();

        let sockets = sys::socket::socketpair(
            sys::socket::AddressFamily::Unix,
            sys::socket::SockType::Datagram,
            None,
            sys::socket::SockFlag::empty())?;

        fcntl::fcntl(sockets.1.as_raw_fd(), fcntl::F_SETFD(fcntl::FdFlag::FD_CLOEXEC))?;

        let mut workers = vec![];

        let mut cmd = Self::build_worker_command(config, &sockets.0.as_raw_fd())?;
        dbg!(&cmd);

        for _ in 0..n {
            let child = cmd.spawn()?;
            workers.push(child);
        }

        Ok(Controller {
            sockets,
            workers,
        })
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

    pub fn do_work(config: options::Config) -> Result<()> {
        let mut control = Controller::spawn(&config)?;

        let mut inodes_seen = handlers::inodes_seen();

        for input_path in &config.args {
            if let Err(err) = handlers::process_file_or_dir_with_func(
                &|already_seen, input_path| {
                    let arg = input_path.to_str().unwrap().as_bytes();
                    unistd::write(&control.sockets.1, arg)?;
                    Ok(false)  // FIXME: pass back modification status
                },
                &mut inodes_seen,
                input_path)
            {
                warn!("{}: failed to process: {}", input_path.display(), err);
            }
        }

        control.close()
    }
}

fn do_worker_work(config: options::Config) -> Result<()> {
    let socket = config.socket.unwrap();
    let socket = unsafe{ UnixDatagram::from_raw_fd(socket) };

    let config = Rc::new(config);
    let handlers = handlers::make_handlers(&config)?;

    let mut buf = vec![0; 4096]; // FIXME: use a better limit here?

    loop {
        let input = match socket.recv(buf.as_mut_slice()) {
            Err(e) => {
                return Err(anyhow!("recv failed: {}", e));
            },
            Ok(n) => str::from_utf8(&buf[..n])?
        };

        if input.is_empty() {
            info!("Bye!");
            return Ok(());
        }

        debug!("Will process {:?}", input);
        let input_path = PathBuf::from(input);
        let mut already_seen = 0;

        if let Err(e) = handlers::process_file(&handlers, &mut already_seen, &input_path) {
            warn!("{}: failed to process: {}", input_path.display(), e);
        }
    }
}

fn main() -> Result<()> {
    let config = match options::Config::make()? {
        None => { return Ok(()); },
        Some(some) => some
    };

    brp_check(&config)?;

    if let Some(socket) = config.socket {
        debug!("Running as worker on socket {}", socket);
        do_worker_work(config)

    } else if let Some(jobs) = config.jobs {
        debug!("Running as controller with {} workers", jobs);
        Controller::do_work(config)

    } else {
        // We're not the controller
        handlers::do_normal_work(config)
    }
}
