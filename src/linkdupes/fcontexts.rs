/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::Result;
use log::debug;
use std::path::Path;

pub fn lookup_context(
    labels: &selinux::label::Labeler<selinux::label::back_end::File>,
    root: Option<&Path>,
    path: &Path,
) -> Result<String> {

    let mut _path = None;

    let adjusted = if let Some(root) = root {
        let rel = path.strip_prefix(root)?;
        _path = Some(Path::new("/").join(rel));
        _path.as_ref().unwrap()
    } else {
        path
    };

    let label = match labels.look_up_by_path(adjusted, None) {
        Err(e) => {
            debug!("{}: {}", adjusted.display(), e);
            // FIXME: figure out if the error is NotFound
            "<<none>>".to_owned()
        }
        Ok(context) => {
            context.to_c_string()?.unwrap().to_str()?.to_owned()
        }
    };

    debug!("Context: {} -> {}", path.display(), label);

    Ok(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fcontext() {
        let labels = match selinux::label::Labeler::new(&[], false) {
            Err(e) => {
                debug!("Cannot initialize selinux: {e}");
                return;
            }
            Ok(labels) => labels
        };

        assert_eq!(lookup_context(&labels, None, &Path::new("/")).unwrap(),
                   "system_u:object_r:root_t:s0");
    }
}
