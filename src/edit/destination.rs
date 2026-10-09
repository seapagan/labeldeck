//! Effective deck destinations without creating missing directories.
use std::{
    io,
    path::{Path, PathBuf},
};

struct Resolved {
    path: PathBuf,
    #[cfg(any(windows, target_os = "macos", test))]
    ancestor: PathBuf,
}

pub fn same_destination(left: &Path, right: &Path) -> io::Result<bool> {
    let left = resolve(left)?;
    let right = resolve(right)?;
    #[cfg(windows)]
    {
        same_resolved(&left, &right, |_| Ok(true))
    }
    #[cfg(target_os = "macos")]
    {
        same_resolved(&left, &right, case_insensitive)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Ok(left.path == right.path)
    }
}

#[cfg(any(windows, target_os = "macos", test))]
fn same_resolved(
    left: &Resolved,
    right: &Resolved,
    probe: impl Fn(&Path) -> io::Result<bool>,
) -> io::Result<bool> {
    if left.path == right.path {
        return Ok(true);
    }
    if left.ancestor != right.ancestor {
        return Ok(false);
    }
    let comparison = compare_missing_case(&left.path, &right.path)?;
    if !comparison || !probe(&left.ancestor)? {
        return Ok(false);
    }
    Ok(true)
}

#[cfg(any(windows, target_os = "macos", test))]
fn compare_missing_case(left: &Path, right: &Path) -> io::Result<bool> {
    if left.components().count() != right.components().count() {
        return Ok(false);
    }
    let mut ambiguous = false;
    for (left, right) in left.components().zip(right.components()) {
        if left == right {
            continue;
        }
        let left = left.as_os_str().as_encoded_bytes();
        let right = right.as_os_str().as_encoded_bytes();
        if left.is_ascii() && right.is_ascii() {
            if !left.eq_ignore_ascii_case(right) {
                return Ok(false);
            }
        } else {
            // Unicode casing/normalization follows filesystem tables, not
            // Rust's Unicode lowercase rules. Missing paths cannot resolve it.
            ambiguous = true;
        }
    }
    if ambiguous {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "cannot establish Unicode case-alias identity for missing Save destinations",
        ))
    } else {
        Ok(true)
    }
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn case_insensitive(directory: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(directory)?;
    if let Some(result) = parent_case_probe(directory, &metadata)? {
        return Ok(result);
    }
    if let Some(result) =
        child_directory_case_probe(directory, metadata.dev())?
    {
        return Ok(result);
    }
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "could not probe filesystem case aliases without creating a directory",
    ))
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn parent_case_probe(
    directory: &Path,
    metadata: &std::fs::Metadata,
) -> io::Result<Option<bool>> {
    use std::os::unix::fs::MetadataExt;
    // A directory's spelling probes its parent's filesystem. Do not infer
    // a mounted volume's policy from the filesystem containing its mount point.
    if let Some(parent) = directory.parent()
        && std::fs::metadata(parent)?.dev() == metadata.dev()
    {
        return directory_case_alias(directory, metadata);
    }
    Ok(None)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn child_directory_case_probe(
    directory: &Path,
    device: u64,
) -> io::Result<Option<bool>> {
    use std::os::unix::fs::MetadataExt;
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = std::fs::symlink_metadata(entry.path())?;
        // Files can have separately named hard links; directories cannot.
        if metadata.is_dir()
            && metadata.dev() == device
            && let Some(result) =
                directory_case_alias(&entry.path(), &metadata)?
        {
            return Ok(Some(result));
        }
    }
    Ok(None)
}

#[cfg(any(target_os = "macos", all(test, unix)))]
fn directory_case_alias(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> io::Result<Option<bool>> {
    use std::os::unix::{ffi::OsStringExt, fs::MetadataExt};
    let Some(name) = path.file_name() else {
        return Ok(None);
    };
    let mut bytes = name.as_encoded_bytes().to_vec();
    let Some(letter) =
        bytes.iter_mut().find(|byte| byte.is_ascii_alphabetic())
    else {
        return Ok(None);
    };
    *letter ^= 0x20;
    let alias = path.with_file_name(std::ffi::OsString::from_vec(bytes));
    match std::fs::symlink_metadata(alias) {
        Ok(alias) => Ok(Some(
            alias.dev() == metadata.dev() && alias.ino() == metadata.ino(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Some(false))
        }
        Err(error) => Err(error),
    }
}

fn resolve(path: &Path) -> io::Result<Resolved> {
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
                #[cfg(any(windows, target_os = "macos", test))]
                let ancestor = resolved.clone();
                for component in suffix.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(Resolved {
                    path: resolved,
                    #[cfg(any(windows, target_os = "macos", test))]
                    ancestor,
                });
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
