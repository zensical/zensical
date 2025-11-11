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

//! File monitor.

use crossbeam::channel::{unbounded, Receiver, TryIter};
use notify::{
    Config, Event, RecommendedWatcher, RecursiveMode, Result, Watcher,
    WatcherKind,
};
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fmt, fs};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File monitor.
///
/// This is a small convenient wrapper around the [`notify`] crate, which uses
/// a [`crossbeam`] channel to simplify event handling. It also tracks watched
/// paths, normalizing the behavior of all file watcher backends, especially
/// in terms of automatically watching newly created files.
///
/// This implementation checks that the paths passed to the file watcher never
/// overlap, because some file watcher backends might not handle addition and
/// removal of overlapping paths correctly. Thus, if a path that is watched is
/// covered by the path that is added, the former will be unwatched before the
/// latter is watched. This is ensured by maintaining a list of watched paths,
/// which is updated whenever a path is added or removed. Additionally, every
/// path is watched recursively, as it doesn't make sense in our case to watch
/// a directory non-recursively. This also has the nice upside of simplifying
/// the API and implementation.
///
/// In order to better understand how this works, let's assume the monitor is
/// configured to watch three paths: `docs`, `docs/assets`, and `docs/posts`:
///
/// ``` text
/// .
/// └─ docs/
///    ├─ assets/
///    └─ posts/
/// ```
///
/// In this scenario, only `docs` is being actively watched, as the two nested
/// paths are covered by it. Since the monitor has been asked to watch `docs`,
/// it unwatched `docs/assets` and `docs/posts`. Subsequently, when `docs` is
/// unwatched, the two nested paths will be watched. This behavior ensures that
/// each file is only ever watched once, and that the monitor is always in a
/// consistent state, normalizing behavior across file watcher backends.
///
/// # Examples
///
/// ```
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// use zensical_watch::agent::Monitor;
///
/// // Create file monitor and start watching
/// let mut monitor = Monitor::default();
/// monitor.watch(".")?;
/// # Ok(())
/// # }
/// ```
pub struct Monitor {
    /// File watcher.
    watcher: Box<dyn Watcher>,
    /// File watcher backend.
    kind: Kind,
    /// Watched paths.
    paths: BTreeMap<PathBuf, bool>,
    /// Message receiver.
    receiver: Receiver<Result<Event>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Monitor {
    /// Creates a file monitor.
    ///
    /// Normally, it's not necessary to use this function, since the [`Default`]
    /// implementation will set up the [`RecommendedWatcher`]. However, if you
    /// want to use a specific watcher, e.g., the [`PollWatcher`][], you can
    /// use this function to create a file monitor with it.
    ///
    /// [`PollWatcher`]: notify::PollWatcher
    ///
    /// # Panics
    ///
    /// Panics if [`notify`] returns an error on [`Watcher`] creation, as the
    /// file monitor is required for the file agent.
    ///
    /// # Examples
    ///
    /// ```
    /// use notify::{Config, PollWatcher};
    /// use std::time::Duration;
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Define poll interval for polling watcher
    /// let config = Config::default()
    ///     .with_poll_interval(Duration::from_secs(1));
    ///
    /// // Create file monitor with polling watcher
    /// let mut monitor = Monitor::new::<PollWatcher>(config);
    /// ```
    #[must_use]
    pub fn new<W>(config: Config) -> Self
    where
        W: 'static + Watcher,
    {
        let (sender, receiver) = unbounded();

        // Disable following of symbolic links, as the file manager tracks them
        // separately to be able to correctly determine the set of events
        let config = config.with_follow_symlinks(false);
        let h = move |res| {
            match res {
                Ok(event) => filter::<W>(event).map(Ok),
                Err(err) => Some(Err(err)),
            }
            .map(|res| sender.send(res));
        };

        // We deliberately use unwrap here, as the capability to spawn threads
        // is a fundamental requirement of the file monitor
        Self {
            watcher: Box::new(W::new(h, config).unwrap()),
            kind: W::kind(),
            paths: BTreeMap::new(),
            receiver,
        }
    }

    /// Watches the given path, recursively.
    ///
    /// This method will not return an error if the given path is already part
    /// of the list of watched paths, but indicate this with the return value.
    ///
    /// # Errors
    ///
    /// Errors returned by [`notify`] are forwarded. Other than that, the path
    /// that is passed to this method must exist and be accessible, as we have
    /// to canonicalize it before adding it to the list of watched paths. This
    /// is essential to uniquely identify paths across the file system.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn watch<P>(&mut self, path: P) -> Result<bool>
    where
        P: AsRef<Path>,
    {
        let path = fs::canonicalize(path)?;

        // Before adding the given path to the list of watched paths, we check
        // if we're already watching it in order to skip duplicate work
        if let Entry::Vacant(entry) = self.paths.entry(path) {
            // Here, we know that we haven't seen this path, so we add it to
            // the list of watched paths, and reconfigure the watcher
            entry.insert(false);
            self.configure()
        } else {
            Ok(false)
        }
    }

    /// Unwatches the given path.
    ///
    /// This method will not return an error if the given path is already part
    /// of the list of watched paths, but indicate this with the return value.
    ///
    /// # Errors
    ///
    /// Errors returned by [`notify`] are forwarded. Other than that, the path
    /// that is passed to this method must exist and be accessible, as we have
    /// to canonicalize it before adding it to the list of watched paths. This
    /// is essential to uniquely identify paths across the file system.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    ///
    /// // Stop watching
    /// monitor.unwatch(".")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn unwatch<P>(&mut self, path: P) -> Result<bool>
    where
        P: AsRef<Path>,
    {
        let path = fs::canonicalize(path)?;

        // After removing the given path from the list of watched paths, we
        // need to check whether it was covered by another path, so it wasn't
        // actively watched, or it was an actively watched path. In the former
        // case, the list of actively watched paths does not change.
        if self.paths.remove(&path).unwrap_or(false) {
            // Here, we know that the path was an actively watched path, so we
            // immediately unwatch it, and reconfigure the watcher. However, we
            // must account for when the file watcher backend is `kqueue`, which
            // emits mysterious errors. See the `refresh` method for details.
            if self.kind == Kind::Kqueue {
                let _ = self.watcher.unwatch(&path);
            } else {
                self.watcher.unwatch(&path)?;
            }

            // Reconfigure the watcher
            self.configure()
        } else {
            Ok(false)
        }
    }

    /// Refreshes the watched path covering the given path.
    ///
    /// # Errors
    ///
    /// Errors returned by [`notify`] are forwarded. Other than that, the path
    /// that is passed to this method must exist and be accessible, as we have
    /// to canonicalize it before adding it to the list of watched paths. This
    /// is essential to uniquely identify paths across the file system.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    ///
    /// // Refresh watcher
    /// monitor.refresh("Cargo.toml")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn refresh<P>(&mut self, path: P) -> Result<bool>
    where
        P: AsRef<Path>,
    {
        let path = fs::canonicalize(path)?;

        // Short-circuit if the file watcher backend is `fsevents`, as watching
        // of subfolders on creation is natively supported by this backend
        if self.kind == Kind::Fsevent {
            return Ok(true);
        }

        // In order to normalize behavior across all file watcher backends, we
        // need to refresh the watched path covering the given path, unwatching
        // it and then watching it again. Some file watcher backends do not
        // gracefully handle overlap, which is why this is necessary.
        let mut found = false;
        for (prefix, active) in &self.paths {
            if *active && path.starts_with(prefix) {
                // If the file watcher backend is `kqueue`, we must ignore the
                // return value of the unwatch operation, because it might fail
                // with a false positive saying that the watched directory does
                // not exist, which it does. `kqueue`'s remove filename method
                // seems to trigger this error when the events are handed over
                // to the operating system, and then things go berzerk.
                //
                // Related issue on GitHub (which we reported):
                // https://github.com/notify-rs/notify/issues/665
                if self.kind == Kind::Kqueue {
                    let _ = self.watcher.unwatch(prefix);
                } else {
                    self.watcher.unwatch(prefix)?;
                }

                // After unwatching, immediately rewatch the actively watched
                // path, so the backend re-scans this part of the file system
                self.watcher.watch(prefix, RecursiveMode::Recursive)?;

                // Indicate that a covering watched path was found
                found = true;
                break;
            }
        }

        // Return refresh result
        Ok(found)
    }

    /// Clears all messages.
    ///
    /// This method clears all messages from the receiver, effectively dropping
    /// all messages that have not been processed. This can be useful when a new
    /// snapshot of the file system is taken, which is the case when the system
    /// is reconfigured or restarted.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    ///
    /// // Clear all messages
    /// monitor.clear();
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear(&mut self) {
        while self.receiver.try_recv().is_ok() {}
    }

    /// Returns an iterator over all pending messages.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    ///
    /// // Create iterator over file monitor
    /// for message in &monitor {
    ///     println!("Message: {:?}", message);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn iter(&self) -> TryIter<'_, Result<Event>> {
        self.receiver.try_iter()
    }

    /// Configures the file watcher backend.
    ///
    /// This method configures the file watcher by checking all watched paths
    /// and updating the set of actively watched paths. This is necessary when
    /// a path is added or removed, as the set of watched paths might change,
    /// because a path that is added might cover an actively watched path.
    #[allow(clippy::bool_comparison)]
    fn configure(&mut self) -> Result<bool> {
        let mut defer = Vec::new();

        // We need to reconfigure the set of watched paths, as we might have
        // added a path that covers an actively watched path
        let mut watched: Option<&PathBuf> = None;
        for (current, active) in &mut self.paths {
            if watched
                .filter(|prefix| current.starts_with(prefix))
                .is_some()
            {
                // The actively watched path is a prefix of the current path,
                // so a covering path was added, which means we must remove it
                if *active == true {
                    *active = false;

                    // Unwatch immediately, accounting for `kqueue`, which emits
                    // mysterious errors. See the `refresh` method for details.
                    if self.kind == Kind::Kqueue {
                        let _ = self.watcher.unwatch(current);
                    } else {
                        self.watcher.unwatch(current)?;
                    }
                }
            } else {
                // The actively watched path isn't a prefix of the current path,
                // so we must watch the current path if it's not already watched
                if *active == false {
                    *active = true;

                    // Defer watch for upward propagation
                    defer.push(current);
                }

                // Update actively watched path
                watched = Some(current);
            }
        }

        // Watch paths after iteration
        let empty = defer.is_empty();
        for path in defer {
            self.watcher.watch(path, RecursiveMode::Recursive)?;
        }

        // Return configuration result
        Ok(!empty)
    }
}

#[allow(clippy::must_use_candidate)]
impl Monitor {
    /// Returns the file watcher backend.
    #[inline]
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Returns the watched paths.
    #[inline]
    pub fn paths(&self) -> &BTreeMap<PathBuf, bool> {
        &self.paths
    }

    /// Returns the underlying receiver.
    #[inline]
    pub fn as_receiver(&self) -> &Receiver<Result<Event>> {
        &self.receiver
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<'a> IntoIterator for &'a Monitor {
    type Item = Result<Event>;
    type IntoIter = TryIter<'a, Self::Item>;

    /// Creates an iterator over the file monitor.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor and start watching
    /// let mut monitor = Monitor::default();
    /// monitor.watch(".")?;
    ///
    /// // Create iterator over file monitor
    /// for message in &monitor {
    ///     println!("Message: {:?}", message);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    fn into_iter(self) -> TryIter<'a, Self::Item> {
        self.iter()
    }
}

// ----------------------------------------------------------------------------

impl Default for Monitor {
    /// Creates a file monitor with the recommended watcher.
    ///
    /// This method creates a file monitor by using the [`RecommendedWatcher`],
    /// which is platform-dependent. In order to create a file monitor using a
    /// specific watcher, e.g., the [`PollWatcher`][], [`Monitor::new`] can
    /// be used instead.
    ///
    /// [`PollWatcher`]: notify::PollWatcher
    ///
    /// # Panics
    ///
    /// Panics if [`notify`] returns an error on [`Watcher`] creation, as the
    /// file monitor is required for the file agent.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_watch::agent::Monitor;
    ///
    /// // Create file monitor
    /// let mut monitor = Monitor::default();
    /// ```
    #[inline]
    fn default() -> Self {
        Self::new::<RecommendedWatcher>(Config::default())
    }
}

// ----------------------------------------------------------------------------

impl fmt::Debug for Monitor {
    /// Formats the file monitor for debugging.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Monitor")
            .field("kind", &self.kind)
            .field("paths", &self.paths)
            .field("receiver", &self.receiver)
            .finish_non_exhaustive()
    }
}

// ----------------------------------------------------------------------------
// Type aliases
// ----------------------------------------------------------------------------

/// File watcher backend.
pub type Kind = WatcherKind;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Filters a file event, checking whether it should be forwarded or not. This
/// function is parametrized over the watcher, so the compiler can optimize it.
#[inline]
fn filter<W>(event: Event) -> Option<Event>
where
    W: 'static + Watcher,
{
    // Unfortunately, the `kqueue` file watcher backend spuriously emits paths
    // that were not actually touched if changes are detected inside symbolic
    // links, which is why we must check for them and ignore them. Only perform
    // this check in case of `kqueue`, as it's not necessary for other backends.
    //
    // Related issue on GitHub:
    // https://github.com/notify-rs/notify/issues/644
    if let Kind::Kqueue = W::kind() {
        let mut iter = event.paths.iter();
        iter.all(|path| {
            // In case the path is not a symbolic link itself, we check if it's
            // a path located inside a symbolic link, as we must ignore those
            if path.is_symlink() {
                true
            } else {
                fs::canonicalize(path).map_or(true, |check| check == *path)
            }
        })
    } else {
        true
    }
    .then_some(event)
}
