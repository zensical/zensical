// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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

//! File event.

use std::fs::FileType;
use std::path::PathBuf;
use std::sync::Arc;

// ----------------------------------------------------------------------------
// Enums
// ----------------------------------------------------------------------------

/// File kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// File.
    File,
    /// Folder.
    Folder,
    /// Symbolic link.
    Link,
}

// ----------------------------------------------------------------------------

/// File event.
#[derive(Clone, Debug)]
pub enum Event {
    /// Creation event.
    Create {
        /// File kind.
        kind: Kind,
        /// File path.
        path: Arc<PathBuf>,
    },

    /// Modification event.
    Modify {
        /// File kind.
        kind: Kind,
        /// File path.
        path: Arc<PathBuf>,
    },

    /// Rename event.
    Rename {
        /// File kind.
        kind: Kind,
        /// File source path.
        from: Arc<PathBuf>,
        /// File target path.
        to: Arc<PathBuf>,
    },

    /// Removal event.
    Remove {
        /// File kind.
        kind: Kind,
        /// File path.
        path: Arc<PathBuf>,
    },
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Event {
    /// Returns the file kind of the event.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Event::Create { kind, .. } => *kind,
            Event::Modify { kind, .. } => *kind,
            Event::Rename { kind, .. } => *kind,
            Event::Remove { kind, .. } => *kind,
        }
    }
    /// Returns the file path of the event.
    ///
    /// Note that the returned path is wrapped in an [`Arc`] for reasons of
    /// efficiency, as the file manager just returns references to paths.
    #[must_use]
    pub fn path(&self) -> Arc<PathBuf> {
        Arc::clone(match self {
            Event::Create { path, .. } => path,
            Event::Modify { path, .. } => path,
            Event::Rename { to, .. } => to,
            Event::Remove { path, .. } => path,
        })
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl From<FileType> for Kind {
    /// Converts a file type to a file kind.
    fn from(value: FileType) -> Self {
        if value.is_dir() {
            Kind::Folder
        } else if value.is_symlink() {
            Kind::Link
        } else {
            Kind::File
        }
    }
}
