// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Canonical physical roots for logical paths.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;

use super::{PathError, SitePath, SourcePath};

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// Error converting an existing physical source below its configured root.
#[derive(Debug, Error)]
pub enum RootError {
    /// The path could not be resolved physically.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The physical source does not belong to this root.
    #[error("source path '{}' is outside root '{}'", path.display(), root.display())]
    Outside { root: PathBuf, path: PathBuf },
    /// The relative source cannot be represented as a logical path.
    #[error(transparent)]
    Logical(#[from] PathError),
}

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Canonical physical root of one provider source relation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRoot(Arc<Path>);

// ----------------------------------------------------------------------------

/// Canonical physical root of the generated site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputRoot(Arc<Path>);

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl SourceRoot {
    /// Canonicalizes an existing source directory once.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        canonical_directory(path).map(|path| Self(Arc::from(path)))
    }

    /// Returns the canonical physical root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Resolves a logical source below this root.
    #[must_use]
    pub fn join(&self, path: &SourcePath) -> PathBuf {
        self.0.join(path.as_str())
    }

    /// Converts an existing physical source below this root.
    ///
    /// Watcher removals need provider-bound logical identities because a
    /// deleted path can no longer be canonicalized. This method is therefore
    /// intentionally limited to existing paths.
    pub fn relative_existing(
        &self, path: impl AsRef<Path>,
    ) -> Result<SourcePath, RootError> {
        let path = fs::canonicalize(path)?;
        let relative = path.strip_prefix(self.as_path()).map_err(|_| {
            RootError::Outside {
                root: self.as_path().to_owned(),
                path: path.clone(),
            }
        })?;
        Ok(SourcePath::from_path(relative)?)
    }
}

// ----------------------------------------------------------------------------

impl OutputRoot {
    /// Creates and canonicalizes the output directory once.
    pub fn prepare(path: impl AsRef<Path>) -> io::Result<Self> {
        fs::create_dir_all(path.as_ref())?;
        canonical_directory(path).map(|path| Self(Arc::from(path)))
    }

    /// Returns the canonical physical root.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Resolves one validated site path below this root lexically.
    #[must_use]
    pub fn join(&self, path: &SitePath) -> PathBuf {
        self.0.join(path.as_str())
    }

    /// Atomically writes a file without following symlinked output parents.
    pub fn write(&self, path: &SitePath, contents: &[u8]) -> io::Result<()> {
        self.write_with(path, |file| file.write_all(contents))
    }

    /// Writes generated data through a temporary file below this root.
    pub fn write_with(
        &self, path: &SitePath,
        write: impl FnOnce(&mut fs::File) -> io::Result<()>,
    ) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let parent = self.parent(path, true)?;
        let target = parent.join(path.file_name());
        let (temporary, mut file) = loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let temporary = parent.join(format!(
                ".zensical-{}-{sequence}.tmp",
                std::process::id()
            ));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };

        let result = write(&mut file).and_then(|()| file.flush());
        drop(file);
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }

    /// Copies a file into the output tree without following output symlinks.
    pub fn copy(
        &self, path: &SitePath, source: impl AsRef<Path>,
    ) -> io::Result<()> {
        let mut source = fs::File::open(source)?;
        self.write_with(path, |target| {
            io::copy(&mut source, target).map(|_| ())
        })
    }

    /// Removes a file while refusing to traverse symlinked output parents.
    pub fn remove(&self, path: &SitePath) -> io::Result<()> {
        let parent = match self.parent(path, false) {
            Ok(parent) => parent,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        match fs::remove_file(parent.join(path.file_name())) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Creates missing parents and rejects symlinks or non-directory parents.
    fn parent(&self, path: &SitePath, create: bool) -> io::Result<PathBuf> {
        let mut parent = self.0.to_path_buf();
        let mut components = path.components().peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                break;
            }
            parent.push(component);
            let metadata = match fs::symlink_metadata(&parent) {
                Ok(metadata) => metadata,
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound && create =>
                {
                    match fs::create_dir(&parent) {
                        Ok(()) => {}
                        Err(error)
                            if error.kind() == io::ErrorKind::AlreadyExists => {
                        }
                        Err(error) => return Err(error),
                    }
                    fs::symlink_metadata(&parent)?
                }
                Err(error) => return Err(error),
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "refusing output through symlink or non-directory parent '{}'",
                        parent.display()
                    ),
                ));
            }
            let resolved = fs::canonicalize(&parent)?;
            if !resolved.starts_with(self.as_path()) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "output parent '{}' escapes root '{}'",
                        resolved.display(),
                        self.as_path().display()
                    ),
                ));
            }
            parent = resolved;
        }
        Ok(parent)
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Canonicalizes an existing directory.
fn canonical_directory(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("path is not a directory: {}", path.display()),
        ))
    }
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    use super::{OutputRoot, RootError, SitePath, SourceRoot};

    #[test]
    fn converts_existing_sources_without_lossy_round_trips() {
        let directory = tempdir().unwrap();
        let root = SourceRoot::open(directory.path()).unwrap();
        let path = directory.path().join("guide/café.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# Page").unwrap();

        let source = root.relative_existing(&path).unwrap();
        assert_eq!(source.as_str(), "guide/café.md");
        assert_eq!(root.join(&source), fs::canonicalize(path).unwrap());
    }

    #[test]
    fn rejects_sources_outside_their_root() {
        let root_dir = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let path = outside.path().join("page.md");
        fs::write(&path, "# Page").unwrap();
        let root = SourceRoot::open(root_dir.path()).unwrap();

        assert!(matches!(
            root.relative_existing(path),
            Err(RootError::Outside { .. })
        ));
    }

    #[test]
    fn prepares_one_output_root_for_site_paths() {
        let directory = tempdir().unwrap();
        let site = directory.path().join("nested/site");
        let root = OutputRoot::prepare(&site).unwrap();
        let path = "assets/app.js".parse::<SitePath>().unwrap();

        assert_eq!(root.as_path(), fs::canonicalize(&site).unwrap());
        assert_eq!(root.join(&path), root.as_path().join("assets/app.js"));
    }
}
