//! Project root lookup for file buffers and buffers without a visited file.

use std::io;
use std::path::{Path, PathBuf};

/// Finds the nearest `.git` directory or file, falling back to the buffer's
/// directory. Relative file names are resolved against `working_directory`.
pub fn root_for(file: Option<&Path>, working_directory: &Path) -> io::Result<PathBuf> {
    let directory = match file {
        Some(file) => {
            let path = crate::files::expand(file, working_directory)?;
            path.parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "File has no parent"))?
                .to_path_buf()
        }
        None => crate::files::expand(Path::new("."), working_directory)?,
    };
    Ok(directory
        .ancestors()
        .find(|candidate| {
            std::fs::metadata(candidate.join(".git"))
                .is_ok_and(|metadata| metadata.is_file() || metadata.is_dir())
        })
        .unwrap_or(&directory)
        .to_path_buf())
}

/// Checks the fixed root itself; this does not rediscover a different project.
pub fn is_available(root: &Path) -> bool {
    root.is_dir()
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
