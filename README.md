# Build postprocessor to reset metadata fields for build reproducibility

This crate provides a binary `add-determinism` that one or more paths,
and will recursively process those paths,
attempting to run the handlers on any files with extensions that match.

For each processed file, a temporary file is opened,
the contents are rewritten,
the modification timestamp is copied from the original file to the temporary copy,
and the copy is renamed over the original.

If processing fails, a warning is emitted,
but no modifications are made and the program returns success.

## Processors

### `ar`

Accepts `*.a`.

Resets the embedded modification time to $SOURCE_DATE_EPOCH and owner:group to 0:0.

### `pyc`

Accepts `*.pyc`.

Uses the [MarshalParser Python module](https://github.com/fedora-python/marshalparser)
to clean up the internal Python object serialization in cache files.

## Notes

This project is inspired by
[strip-nondeterminism](https://salsa.debian.org/reproducible-builds/strip-nondeterminism),
but is written from scratch in Rust.
For Debian, build tools are written in Perl and more Perl is not an issue.
But in Fedora/RHEL/…, tools are written in Bash, Python, or compiled,
and we don't want to pull in Perl into all buildroots.
