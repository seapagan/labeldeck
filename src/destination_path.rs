//! Read-only resolution through the nearest existing destination ancestor.
use std::{
    io,
    path::{Path, PathBuf},
};

/// Return the resolved path and its canonical existing ancestor, preserving
/// missing components verbatim for the caller's filesystem alias checks.
pub(crate) fn resolve(path: &Path) -> io::Result<(PathBuf, PathBuf)> {
    let mut ancestor = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(&ancestor) {
            Ok(_) => {
                let mut resolved = std::fs::canonicalize(&ancestor)?;
                // Windows may report NotFound below a file. A missing suffix
                // is valid only beneath a directory (following parent links).
                if !suffix.is_empty()
                    && !std::fs::metadata(&resolved)?.is_dir()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        format!(
                            "destination ancestor {} is not a directory",
                            ancestor.display()
                        ),
                    ));
                }
                let ancestor = resolved.clone();
                for component in suffix.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok((resolved, ancestor));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "could not resolve missing destination ancestor",
                        )
                    })?
                    .to_owned();
                suffix.push(name);
                if !ancestor.pop() {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
}
