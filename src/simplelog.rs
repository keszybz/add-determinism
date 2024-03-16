/* SPDX-License-Identifier: GPL-3.0-or-later */

use log::debug;
use std::io::{self, Write};

pub struct SimpleLog {
    maxlevel: log::LevelFilter,
}

impl SimpleLog {
    /// Create new kernel logger with error level filter
    pub fn with_level(level: log::LevelFilter) -> SimpleLog {
        SimpleLog {
            maxlevel: level,
        }
    }
}

fn _write_stdout(record: &log::Record) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{}", record.args());
    let _ = stdout.flush();
}

impl log::Log for SimpleLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= self.maxlevel
    }

    fn log(&self, record: &log::Record) {
        if record.level() > self.maxlevel {
            return;
        }

        _write_stdout(record);
    }

    fn flush(&self) {}
}

/// Set up logger with specified error level as the default logger
pub fn init_with_level(level: log::LevelFilter) -> Result<(), log::SetLoggerError> {
    log::set_boxed_logger(Box::new(SimpleLog::with_level(level)))?;
    log::set_max_level(level);

    debug!("Initialized logging with log level {}", level);

    Ok(())
}
