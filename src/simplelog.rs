/* SPDX-License-Identifier: GPL-3.0-or-later */

use log::debug;
use std::io::{self, Write};
use std::process;

pub struct SimpleLog {
    level: log::LevelFilter,
    show_pid: bool,
}

impl SimpleLog {
    /// Create new kernel logger with error level filter
    pub fn new(level: log::LevelFilter, show_pid: bool) -> SimpleLog {
        SimpleLog {
            level,
            show_pid,
        }
    }
}

impl log::Log for SimpleLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if record.level() > self.level {
            return;
        }

        let mut stdout = io::stdout().lock();
        if self.show_pid {
            let _ = writeln!(stdout, "[{}] {}", process::id(), record.args());
        } else {
            let _ = writeln!(stdout, "{}", record.args());
        };
        let _ = stdout.flush();
    }

    fn flush(&self) {}
}

/// Set up logger with specified error level as the default logger
pub fn init(level: log::LevelFilter, show_pid: bool) -> Result<(), log::SetLoggerError> {
    log::set_boxed_logger(Box::new(SimpleLog::new(level, show_pid)))?;
    log::set_max_level(level);

    debug!("Initialized logging with log level {level}");

    Ok(())
}
