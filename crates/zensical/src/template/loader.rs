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

//! MiniJinja template engine.

use minijinja::{Error, ErrorKind};
use std::path::PathBuf;
use std::{fs, io};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// MiniJinja template loader with override support.
pub struct Loader {
    /// Template search directories.
    dirs: Vec<PathBuf>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Loader {
    /// Creates a template loader.
    pub fn new<I>(dirs: I) -> Self
    where
        I: IntoIterator<Item = PathBuf>,
    {
        Self {
            dirs: dirs.into_iter().collect(),
        }
    }

    /// Loads a template by name, searching all configured directories.
    pub fn load<S>(&self, name: S) -> Result<Option<String>, Error>
    where
        S: AsRef<str>,
    {
        for dir in &self.dirs {
            match fs::read_to_string(dir.join(name.as_ref())) {
                Ok(res) => return Ok(Some(res)),
                Err(err) if err.kind() == io::ErrorKind::NotFound => {
                    // Try next directory
                }
                Err(err) => {
                    let inner = Error::new(
                        ErrorKind::InvalidOperation,
                        "could not read template",
                    );
                    return Err(inner.with_source(err));
                }
            }
        }

        // No template found
        Ok(None)
    }
}
