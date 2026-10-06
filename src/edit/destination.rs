//! Effective deck destinations without creating missing directories.
use std::{
    io,
    path::{Path, PathBuf},
};

pub fn same_destination(left: &Path, right: &Path) -> io::Result<bool> {
    let left = resolve(left)?;
    let right = resolve(right)?;
    #[cfg(windows)]
    {
        Ok(left
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy()))
    }
    #[cfg(not(windows))]
    {
        Ok(left == right)
    }
}

fn resolve(path: &Path) -> io::Result<PathBuf> {
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
                for component in suffix.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
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

#[cfg(test)]
mod tests;
