//! Effective deck destinations without creating missing directories.
use std::{
    io,
    path::{Path, PathBuf},
};

struct Resolved {
    path: PathBuf,
    #[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
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
    #[cfg(target_os = "linux")]
    {
        same_linux(&left, &right)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        Ok(left.path == right.path)
    }
}

#[cfg(target_os = "linux")]
fn same_linux(left: &Resolved, right: &Resolved) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    if left.path == right.path {
        return Ok(true);
    }
    let left_meta = std::fs::metadata(&left.ancestor)?;
    let right_meta = std::fs::metadata(&right.ancestor)?;
    if (left_meta.dev(), left_meta.ino())
        != (right_meta.dev(), right_meta.ino())
    {
        return Ok(false);
    }
    if left_meta.is_file() {
        return same_linux_entry(&left.path, &right.path);
    }
    let left_suffix = left
        .path
        .strip_prefix(&left.ancestor)
        .map_err(io::Error::other)?;
    let right_suffix = right
        .path
        .strip_prefix(&right.ancestor)
        .map_err(io::Error::other)?;
    if left_suffix == right_suffix {
        return Ok(true);
    }
    if compare_missing_case(left_suffix, right_suffix)? {
        return Err(uncertain_linux_alias());
    }
    Ok(false)
}

#[cfg(target_os = "linux")]
fn uncertain_linux_alias() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "cannot establish Save directory-entry identity on a potentially casefolded Linux directory",
    )
}

#[cfg(target_os = "linux")]
fn same_linux_entry(left: &Path, right: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    let left_parent = left.parent().ok_or_else(uncertain_linux_alias)?;
    let right_parent = right.parent().ok_or_else(uncertain_linux_alias)?;
    let a = std::fs::metadata(left_parent)?;
    let b = std::fs::metadata(right_parent)?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Ok(false); // Distinct hard-link entries; rename replaces only one.
    }
    if left.file_name() == right.file_name() {
        return Ok(true);
    }
    let names = std::fs::read_dir(left_parent)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<io::Result<Vec<_>>>()?;
    distinct_linux_entries(left.file_name(), right.file_name(), &names)
}

#[cfg(target_os = "linux")]
fn distinct_linux_entries(
    left: Option<&std::ffi::OsStr>,
    right: Option<&std::ffi::OsStr>,
    names: &[std::ffi::OsString],
) -> io::Result<bool> {
    let exact =
        |name| names.iter().any(|entry| Some(entry.as_os_str()) == name);
    if exact(left) && exact(right) {
        Ok(false)
    } else {
        // A shared inode with different lookup spellings may be one casefolded
        // entry or distinct hard links. Refuse rather than equating the paths.
        Err(uncertain_linux_alias())
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

#[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
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
    let (path, _ancestor) = crate::destination_path::resolve(path)?;
    Ok(Resolved {
        path,
        #[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
        ancestor: _ancestor,
    })
}

#[cfg(test)]
mod tests;
