/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{anyhow, bail, Result};
use chrono::{TimeZone, Utc};
use log::{debug, log, warn, Level};
use std::env;
use std::path::{Path, PathBuf};
use std::time;

pub fn source_date_epoch() -> Result<Option<i64>> {
    let mut source_date_epoch = match env::var("SOURCE_DATE_EPOCH") {
        Ok(val) => Some(val.parse::<i64>()?),
        Err(_) => None,
    };

    if let Some(v) = source_date_epoch {
        let now = time::SystemTime::now();
        let now_sec = now.duration_since(time::UNIX_EPOCH).unwrap().as_secs();

        let neg = v < 0;
        let pos = v > 0 && v as u64 > now_sec;

        log!(if neg || pos { Level::Warn } else { Level::Debug },
             "SOURCE_DATE_EPOCH timestamp: {v} ({})",
             Utc.timestamp_opt(v, 0).unwrap());
        if neg {
            warn!("SOURCE_DATE_EPOCH timestamp is negative, ignoring: {v}");
            source_date_epoch = None;
        } else if pos {
            warn!("SOURCE_DATE_EPOCH timestamp is in the future: {v} > {now_sec}");
        }
    } else {
        debug!("SOURCE_DATE_EPOCH timestamp: {}", "(unset)");
    }

    Ok(source_date_epoch)
}

pub fn brp_check(
    build_root: Option<String>,
    inputs: &Vec<PathBuf>,
) -> Result<()> {

    let build_root = build_root.map_or_else(
        || env::var("RPM_BUILD_ROOT")
            .map_err(|e| anyhow!("$RPM_BUILD_ROOT is not set correctly: {e}")),
        Ok,
    )?;

    if build_root.is_empty() {
        bail!("Empty $RPM_BUILD_ROOT is not allowed");
    }

    // Canonicalize the path, removing duplicate or trailing slashes
    // and intermediate dot components, but not double dots.
    let build_root = PathBuf::from_iter(Path::new(&build_root).iter());

    if build_root == Path::new("/") {
        bail!("RPM_BUILD_ROOT={build_root:?} is not allowed");
    }

    for arg in inputs {
        if !arg.starts_with(&build_root) {
            bail!("Path {arg:?} does not start with RPM_BUILD_ROOT={build_root:?}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_brp_check() {
        let inputs = vec![
            "/var/tmp/foo/bar".into(),
            "/var/tmp/foo/./bar".into(),
            // Sic, this is allowed.
            "/var/tmp/foo/./bar/../asdf".into(),
            "/var/tmp/foo/./bar/../../../asdf".into(),
        ];

        assert!(brp_check(Some("".to_string()), &inputs).is_err());
        assert!(brp_check(Some("///.///".to_string()), &inputs).is_err());
        assert!(brp_check(Some("/var/tmp/foo2".to_string()), &inputs).is_err());
        assert!(brp_check(Some("/var/tmp/foo///./".to_string()), &inputs).is_ok());
    }
}
