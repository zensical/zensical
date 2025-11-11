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

//! File manager.

use ahash::{HashMap, HashSet};
use file_id::FileId;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::{fs, io};
use walkdir::{DirEntry, WalkDir};

use super::event::{Event, Kind};
use super::Result;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// File manager.
///
/// The file manager represents a constituted effort to normalize events across
/// different file watcher backends, which all exhibit their own behaviors and
/// do not reliably emit the same events. This is particularly challenging for
/// symbolic links, as well as events that create deeply nested structures.
///
/// It tries to be a better version of the debouncer implementation provided by
/// the [`notify`] crate, but is significantly more complex. Note that symbolic
/// links are explicitly tracked to consistently propagate changes of files or
/// folders that reside inside them to all other instances. While all backends
/// supported by [`notify`] itself are handled by this implementation, there
/// might be some problems with symbolic links inside Docker.
///
/// Note that every [`PathBuf`] is wrapped in an [`Arc`] to reduce memory usage,
/// as the file manager requires multiple copies of the same path in order to
/// accurately track locations, identifiers and symbolic links. By using shared
/// references, memory usage is reduced by a factor of 3 or more. Also note that
/// this needs to be an [`Arc`] and not an [`Rc`][], as the file agent, which
/// controls the file monitor and manager, runs on a separate thread.
///
/// [`Rc`]: std::rc::Rc
///
/// # Features
///
/// - Automatically watches files and folders in a given directory
/// - Automatically tracks file system events at arbitrary depths
/// - Propagates folder renames to all files and folders inside it
/// - Propagates events to all instances of a symbolic link
/// - Limits symbolic links to actively watched paths for security
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use zensical_watch::agent::Manager;
///
/// // Create file manager
/// let mut manager = Manager::new();
///
/// // Handle and register paths
/// for result in manager.handle(["."]) {
///     println!("{:?}", result);
/// }
/// ```
#[derive(Debug, Default)]
pub struct Manager {
    /// File paths map.
    paths: BTreeMap<Arc<PathBuf>, (FileId, Kind)>,
    /// Symbolic links map.
    links: BTreeMap<Arc<PathBuf>, Vec<Arc<PathBuf>>>,
    /// File identifiers map.
    ids: HashMap<FileId, Arc<PathBuf>>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Manager {
    /// Creates a file manager.
    ///
    /// # Examples
    ///
    /// ```
    /// use zensical_watch::agent::Manager;
    ///
    /// // Create file manager
    /// let manager = Manager::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Handles a set of paths and generates events.
    ///
    /// This method takes an iterator of paths, and then, depending on whether
    /// the files, folders or symbolic links referred to by those paths still
    /// exist, deduces the corresponding events from those paths and the
    /// the internal state of the manager.
    ///
    /// The manager keeps track of all paths and file identifiers, as there's
    /// no other way to accurately determine what kind of event has occurred,
    /// since file watcher backends are not consistent in behavior, especially
    /// for symbolic links. Sadly, it's really a huge mess. This is also why
    /// the manager queries the file system itself, to learn which files still
    /// exist, and which have been renamed or removed. We do our best to track
    /// and determine what has happened, but it's yet not perfect.
    ///
    /// Note that the given paths are deduplicated, as they are expected to be
    /// extracted from a series of [`notify`] events, which are buffered before
    /// they're passed to this function, so renames are not split into removals
    /// and creations. The manager tries to make sure all events are accurate.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use zensical_watch::agent::Manager;
    ///
    /// // Create file manager
    /// let mut manager = Manager::new();
    ///
    /// // Handle and register paths
    /// for result in manager.handle(["."]) {
    ///     println!("{:?}", result);
    /// }
    /// ```
    pub fn handle<T>(&mut self, paths: T) -> Vec<Result<Event>>
    where
        T: IntoIterator,
        T::Item: Into<PathBuf>,
    {
        let mut results = Vec::new();
        let mut changes = BTreeMap::new();

        // 1st pass: filter out all paths that point to files or folders that
        // exist, and associate them with their OS-dependent file identifiers.
        // Also, only keep unique paths, as some file watcher backends emit
        // multiple events, but we use a vector to preserve the ordering.
        let mut once = HashSet::default();
        let paths = paths
            .into_iter()
            .map(Into::into)
            .filter(|path| once.insert(path.clone()))
            .filter_map(|path| {
                // If the path points to a file or folder, the event is either
                // a creation or modification, or the target path of a rename
                let Ok(id) = get_file_id(&path) else {
                    return Some(path);
                };

                // Usually, there's no previous entry when inserting a new path.
                // However, some file watcher backends like `kqueue` might emit
                // events for paths inside symbolic links, which is when there
                // already is an entry for the given file identifier.
                match changes.entry(id) {
                    Entry::Vacant(entry) => {
                        entry.insert(path);
                    }

                    // By canonicalizing the path, and checking whether it's
                    // the same as the original, we check whether the path is
                    // inside a symbolic link. If the path can be canonicalized
                    // and is different from the previous one, we replace it.
                    Entry::Occupied(mut entry) => {
                        if let Ok(to) = fs::canonicalize(&path) {
                            if *entry.get() != to {
                                entry.insert(path);
                            }
                        }
                    }
                }

                // Return nothing, as the path was consumed
                None
            })
            .collect::<Vec<_>>();

        // 2nd pass: filter out all non-existing paths that we can match to a
        // previously seen path, coalescing them into a rename event
        let paths = paths
            .into_iter()
            .filter_map(|path| {
                // If we've already seen the path, and we got another path with
                // the same file identifier in this iteration, we know that the
                // path was renamed, and we can coalesce the two events into a
                // single rename instead of a removal and creation
                if let Some((id, _)) = self.paths.get(&path) {
                    if let Some(to) = changes.remove(id) {
                        results.append(&mut self.handle_rename(&to));
                        return None;
                    }
                }

                // The path does not point to a file or folder, and it's also
                // not part of a rename, so we know that it must be a removal
                Some(path)
            })
            .collect::<Vec<_>>();

        // We now know that the remaining changes are either file or folder
        // creations or modifications, depending on if we've seen them before
        for path in changes.into_values() {
            if self.paths.contains_key(&path) {
                results.append(&mut self.handle_modify(&path));
            } else {
                results.append(&mut self.handle_create(&path));
            }
        }

        // All other remaining paths correspond to removals, but we can ignore
        // those that we haven't seen before. This might happen when an editor
        // creates a temporary file and removes it in the same iteration, e.g.,
        // like vim's infamous 4913 file, or in case of paths that are inside
        // symbolic links which are not actively monitored, as for reasons of
        // security, we don't just always resolve symbolic links.
        for path in paths {
            if self.paths.contains_key(&path) {
                results.append(&mut self.handle_remove(&path));
            }
        }

        // After processing all paths, we need to check if a path refers to a
        // file or folder that is referenced transitively through a monitored
        // symbolic link. This must be done before considering symbolic links
        // inside the results themselves, or we'll generate duplicate events.
        let mut inserts = Vec::new();
        if !self.links.is_empty() {
            // In case the event doesn't contain a path that is a symbolic link
            // itself, try to spread it to all symbolic links, if inside any
            for (i, result) in results.iter().enumerate() {
                if let Ok(event) = result {
                    if event.kind() != Kind::Link {
                        inserts.push((i, self.spread(event)));
                    }
                }
            }

            // Insert results from spreaded symbolic links at the position of
            // the original result, while ensuring that indices remain valid by
            // iterating in reverse. Each followed symbolic link includes the
            // original result itself.
            for (i, insert) in inserts.drain(..).rev() {
                results.splice(i..=i, insert);
            }
        }

        // Follow symbolic links and list all files and folders inside them, if
        // and only if they are actively monitored (for security reasons). Like
        // mentioned above, this must be done after spreading symbolic links,
        // or we'll end up with duplicate events in the result set.
        for (i, result) in results.iter().enumerate() {
            if let Ok(event) = result {
                if event.kind() == Kind::Link {
                    inserts.push((i, self.follow(event)));
                }
            }
        }

        // Lastly, handle results generated from following symbolic links that
        // are part of the original set of results, and insert them accordingly
        for (i, insert) in inserts.into_iter().rev() {
            results.splice(i..=i, insert);
        }

        // Return results, including errors
        results
    }

    /// Handles a creation event.
    fn handle_create(&mut self, root: &PathBuf) -> Vec<Result<Event>> {
        let iter = walk(root).filter_map(|item| {
            item.and_then(|entry| {
                let kind = entry.file_type();
                let path = entry.into_path();

                // In case the path refers to a folder, we're enumerating files
                // recursively. However, since some file watcher backends will
                // recurse as well, we might have already encountered the path
                // in a previous iteration, so we can just skip it here.
                if self.paths.contains_key(&path) {
                    return Ok(None);
                }

                // Theoretically, obtaining the file identifier should not fail
                // at this point, but operating systems can be unpredictable
                let id = get_file_id(&path)?;

                // Here, we know that we're looking at a new file, so we need
                // to retrieve the file type and materialize its path
                let kind = Kind::from(kind);
                let path = Arc::new(path);

                // We record the path and file identifier association in both
                // directions, so we can accurately track all events
                self.paths.insert(Arc::clone(&path), (id, kind));
                self.ids.insert(id, Arc::clone(&path));

                // Return event
                Ok(Some(Event::Create { kind, path }))
            })
            .transpose()
        });

        // Collect results from iterator
        iter.collect()
    }

    /// Handles a modification event.
    fn handle_modify(&mut self, root: &PathBuf) -> Vec<Result<Event>> {
        let stat = self.paths.get(root);
        let iter = stat.into_iter().filter_map(|(id, kind)| {
            // Some file watcher backends like `kqueue` emit modifications for
            // folders, which we're not interested in, so we filter them out
            if *kind == Kind::Folder {
                None
            } else {
                self.ids.get(id).map(|path| {
                    Ok(Event::Modify {
                        kind: *kind,
                        path: Arc::clone(path),
                    })
                })
            }
        });

        // Collect results from iterator
        iter.collect()
    }

    /// Handles a rename event.
    fn handle_rename(&mut self, root: &PathBuf) -> Vec<Result<Event>> {
        let iter = walk(root).filter_map(|item| {
            item.and_then(|entry| {
                let path = entry.path();

                // Better safe than sorry - although we know that the path has
                // just been created, there might be cases where this fails
                let id = get_file_id(path)?;
                if let Some(prev) = self.ids.get_mut(&id) {
                    let path = Arc::new(entry.into_path());
                    let from = Arc::clone(prev);

                    // Rename the path by migrating the file identifier to the
                    // new path, if the previous path existed. If not, ignore.
                    if let Some((id, kind)) = self.paths.remove(prev) {
                        self.paths.insert(Arc::clone(&path), (id, kind));

                        // The `polling` file watcher backend propagates rename
                        // events to files and folders inside of symbolic links,
                        // which is different than all other backends. In case
                        // the file is emitted before the folder in which it is
                        // contained, this will result in the rename of a file
                        // that has already been renamed, which we must ignore.
                        return if path == from {
                            Ok(None)
                        } else {
                            // Update the file identifier map with the new path
                            // and return the rename from source to target path
                            prev.clone_from(&path);
                            Ok(Some(Event::Rename { kind, from, to: path }))
                        };
                    }
                }

                // Return nothing, likely due to a file system error
                Ok(None)
            })
            .transpose()
        });

        // Collect results from iterator
        iter.collect()
    }

    /// Handles a removal event.
    fn handle_remove(&mut self, root: &PathBuf) -> Vec<Result<Event>> {
        // We need to collect all paths that start with the given path, as we
        // can't mutate the file paths map while iterating over it
        let mut paths = Vec::new();
        for (path, _) in self.paths.range(root.clone()..) {
            if path.starts_with(root) {
                paths.push(Arc::clone(path));
            } else {
                break;
            }
        }

        // Next, we remove all collected paths from the file manager, and emit
        // a removal event for each path, removing the path and file identifier
        // association. Note that we iterate the file path map in reverse, as
        // we need to make sure that files are always emitted before folders.
        let iter = paths.into_iter().rev().filter_map(|path| {
            self.paths.remove(&path).and_then(|(id, kind)| {
                self.ids
                    .remove(&id)
                    .map(|path| Ok(Event::Remove { kind, path }))
            })
        });

        // Collect results from iterator
        iter.collect()
    }

    /// Follows a symbolic link after an event.
    ///
    /// This method is only ever called for symbolic links, keeping track of
    /// them, while expanding all paths inside the symbolic link to events. For
    /// more information on how symbolic links are handled, see the example in
    /// the [`Manager::expand`] method.
    #[allow(clippy::bool_comparison)]
    fn follow(&mut self, event: &Event) -> Vec<Result<Event>> {
        debug_assert_eq!(event.kind(), Kind::Link);

        // Update the symbolic links maps, expand all paths inside the symbolic
        // link, and return the results. Depending on the event kind, expansion
        // must happen before or after the symbolic link has been updated, as
        // removal events are handled differently than all other events.
        let mut results = Vec::new();
        match event {
            // Handle a creation event
            Event::Create { path, .. } => {
                let res = fs::canonicalize(path.as_path()).map(|to| {
                    let paths = self.links.entry(Arc::new(to)).or_default();
                    if !paths.contains(path) {
                        paths.push(Arc::clone(path));
                    }
                    event.clone()
                });

                // After updating the symbolic links map, add the original
                // event and expand all paths inside the symbolic link
                results.push(res.map_err(Into::into));
                results.append(&mut self.expand(event));
            }

            // Handle a modification event
            Event::Modify { .. } => {
                // Nothing to be done
            }

            // Handle a rename event
            Event::Rename { from, to: path, .. } => {
                // Obtain the path to rename from the symbolic links map, and
                // update the previous target path with the next path. Once the
                // symbolic link has been updated, the iterator will immediately
                // abort and return an empty option to denote success.
                let done = self.links.iter_mut().find_map(|(_, paths)| {
                    paths.iter().position(|check| check == from).map(|index| {
                        paths[index].clone_from(path);
                    })
                });

                // In case no symbolic link was found, we try to canonicalize
                // the path, and update the symbolic links map if it exists
                let res = match done {
                    Some(()) => Ok(event.clone()),
                    None => fs::canonicalize(path.as_path()).map(|to| {
                        let paths = self.links.entry(Arc::new(to)).or_default();
                        if !paths.contains(path) {
                            paths.push(Arc::clone(path));
                        }
                        event.clone()
                    }),
                };

                // After updating the symbolic links map, add the original
                // event and expand all paths inside the symbolic link
                results.push(res.map_err(Into::into));
                results.append(&mut self.expand(event));
            }

            // Handle a removal event
            Event::Remove { path, .. } => {
                // Expand all paths inside the symbolic link before removal,
                // or path enumeration will not be possible anymore. Note that
                // we reverse the order of events, so that folder contents are
                // always listed before the folder itself.
                results.append(&mut self.expand(event));
                results.reverse();

                // After expanding all paths, we remove the symbolic link from
                // the symbolic links map, including empty vectors of paths
                self.links.retain(|_, paths| {
                    paths.retain(|check| check != path);
                    paths.is_empty() == false
                });

                // Finally, add the original event for the symbolic link at
                // the end of the result set, as it's the last event to emit
                results.push(Ok(event.clone()));
            }
        }

        // Return results, including errors
        results
    }

    /// Expands all paths inside a symbolic link to events.
    ///
    /// This method is only ever called for symbolic links, and is thus a dual
    /// of [`Manager::spread`], which handles files inside of symbolic links.
    /// It is used to expand symbolic links to all paths inside them, which
    /// implements monitored following of symbolic links.
    ///
    /// # Examples
    ///
    /// The following directory structure lists `assets` folders per language,
    /// each of which refer to a `shared` top-level folder for sharing assets:
    ///
    /// ``` text
    /// .
    /// └─ docs/
    ///    ├─ shared/
    ///    │  ├─ image-1.png
    ///    │  ├─ image-2.png
    ///    │  └─ ...
    ///    ├─ en/
    ///    │  ├─ assets/ -> ../shared/
    ///    │  └─ ...
    ///    └─ fr/
    ///       ├─ assets/ -> ../shared/
    ///       └─ ...
    /// ```
    ///
    /// When the symbolic link `docs/en/assets` is created, modified, removed or
    /// renamed, also taking into account if the symbolic link is valid before
    /// and/or after the event, the following paths will be emitted by this
    /// method as part of the corresponding kinds of events:
    ///
    /// - `docs/en/assets/image-1.png`
    /// - `docs/en/assets/image-2.png`
    ///
    /// For instance, if a relative symbolic link is moved to another location
    /// where it becomes invalid, removal events are emitted for those paths.
    fn expand(&self, event: &Event) -> Vec<Result<Event>> {
        debug_assert_eq!(event.kind(), Kind::Link);

        // When the event is not a removal event, for which we'd know for sure
        // that the path cannot be canonicalized, as it does not exist anymore,
        // we resolve the symbolic link to its target to emit errors for all
        // other events, specifically renames
        let root = event.path();
        let broken = match &event {
            Event::Remove { .. } => None,
            _ => fs::canonicalize(root.as_path()).map_err(Into::into).err(),
        };

        // Regardless of whether the target exists, we obtain its path, so we
        // can enumerate all files and folders inside the symbolic link.
        let target = self.links.iter().find_map(|(path, paths)| {
            paths.contains(&root).then_some(Arc::clone(path))
        });

        // Now, enumerate all paths that start with the path of the given event,
        // filtering out the starting path, since it's the symbolic link itself
        let iter = target.into_iter().flat_map(|head| {
            let iter = self.paths.range(Arc::clone(&head)..).skip(1);
            iter.scan((), move |(), (path, (_, kind))| {
                path.strip_prefix(head.as_path())
                    .ok()
                    .map(|tail| (*kind, tail))
            })
        });

        // Check if the next link target is broken, which means that the link
        // could not be canonicalized. If the previous link target was broken
        // as well, we can just ignore the event. In all other cases, we map
        // each path inside the symbolic link to the corresponding event.
        let next = broken.is_none();
        let iter = iter.filter_map(move |(kind, tail)| {
            let path = Arc::new(root.join(tail));

            // Map each path to the same kind of event as the symbolic link,
            // except for renames where one of the targets is broken
            let event = match &event {
                Event::Create { .. } => Some(Event::Create { kind, path }),
                Event::Modify { .. } => Some(Event::Modify { kind, path }),
                Event::Remove { .. } => Some(Event::Remove { kind, path }),
                Event::Rename { from, .. } => {
                    let up = from.parent().expect("invariant");
                    let from = Arc::new(from.join(tail));

                    // Check if the previous link target was broken, which we
                    // can do by canonicalizing it at the previous location
                    let prev = fs::read_link(root.as_path())
                        .and_then(|path| fs::canonicalize(up.join(path)))
                        .is_ok();

                    // Construct event accordingly, based on the existence of
                    // the previous and next target of the symbolic link
                    if prev && next {
                        Some(Event::Rename { kind, from, to: path })
                    } else if prev {
                        Some(Event::Remove { kind, path: from })
                    } else if next {
                        Some(Event::Create { kind, path })
                    } else {
                        None
                    }
                }
            };

            // Return event
            event.map(Ok)
        });

        // Combine error and collect results from iterator
        broken.into_iter().map(Err).chain(iter).collect()
    }

    /// Spreads an event to all symbolic links.
    ///
    /// This method is only ever called if we're monitoring one or more symbolic
    /// links. If an event happened inside a folder that is targetted by one or
    /// more symbolic links, the event is spread across all of them, so it is
    /// correctly propagated to all monitored paths.
    ///
    /// # Examples
    ///
    /// The following directory structure lists `assets` folders per language,
    /// each of which refer to a `shared` top-level folder for sharing assets:
    ///
    /// ``` text
    /// .
    /// └─ docs/
    ///    ├─ shared/
    ///    │  ├─ image-1.png
    ///    │  ├─ image-2.png
    ///    │  └─ ...
    ///    ├─ en/
    ///    │  ├─ assets/ -> ../shared/
    ///    │  └─ ...
    ///    └─ fr/
    ///       ├─ assets/ -> ../shared/
    ///       └─ ...
    /// ```
    ///
    /// When the `image-1.png` file in the top-level `shared` folder is created,
    /// modified, removed or renamed, regardless whether it's moved within the
    /// shared folder, or moved in or out of it, the following paths will be
    /// emitted by this method with the corresponding kinds of events:
    ///
    /// - `docs/shared/image-1.png`
    /// - `docs/en/assets/image-1.png`
    /// - `docs/fr/assets/image-1.png`
    ///
    /// For instance, if a file is moved out of a folder that has symbolic links
    /// pointing to it, the file is removed from all symbolic links.
    fn spread(&self, event: &Event) -> Vec<Result<Event>> {
        debug_assert_ne!(event.kind(), Kind::Link);

        // Select all symbolic links for the given event, if any, so we can map
        // the event to all paths inside the symbolic link in the next step. In
        // case the file is not within a folder that is targetted by a symbolic
        // link, this method returns nothing.
        let root = event.path();
        let select = self.links.iter().find_map(|(path, paths)| {
            root.strip_prefix(path.as_path())
                .ok()
                .map(|tail| (path, paths, tail))
        });

        // Now, enumerate all selected symbolic links, and combine each of its
        // paths with the event, so we can emit the event for each path
        let iter = select.into_iter().flat_map(|(head, paths, tail)| {
            paths.iter().map(move |root| {
                let path = Arc::new(root.join(tail));
                let kind = event.kind();

                // Map each path to the same kind of event as the symbolic link,
                // except for renames where the previous target is broken
                let event = match &event {
                    Event::Create { .. } => Event::Create { kind, path },
                    Event::Modify { .. } => Event::Modify { kind, path },
                    Event::Remove { .. } => Event::Remove { kind, path },
                    Event::Rename { from, .. } => {
                        if let Ok(tail) = from.strip_prefix(head.as_path()) {
                            let from = Arc::new(root.join(tail));
                            Event::Rename { kind, from, to: path }
                        } else {
                            Event::Create { kind, path }
                        }
                    }
                };

                // Return event
                Ok(event)
            })
        });

        // Create original result, and if the previously constructed iterator
        // doesn't yield any results, and the event is a rename event, we know
        // for sure that the target does not exist any more. Thus, we need to
        // emit a removal event for all paths inside the symbolic link.
        let target = Some(Ok(event.clone()));
        let mut iter = iter.peekable();
        if let Event::Rename { kind, from, .. } = event {
            if iter.peek().is_none() {
                let path = Arc::clone(from);

                // Create a temporary removal event, so we can spread it to all
                // paths inside the symbolic link, and then return the original.
                // It's easier to just reuse the removal business logic, as
                // otherwise we'd need more code for an edge case.
                let event = Event::Remove { kind: *kind, path };
                return target
                    .into_iter()
                    .chain(self.spread(&event).into_iter().skip(1))
                    .collect();
            }
        }

        // Collect results from iterator
        target.into_iter().chain(iter).collect()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates a file system iterator from the given path.
///
/// When walking directory trees, we explicitly do not follow symbolic links, as
/// we need to track them explicitly. This is particularly necessary in order to
/// normalize the behavior across different file watcher backends, as all of
/// them treat symbolic links differently.
///
/// Files in a directory are typically not stored sequentially, so they're most
/// likely not returned in lexicographical order. While the hierarchy of files
/// and folders is preserved, the order of files inside of folders is not well
/// defined. Although it's possible to sort the files inside of a folder before
/// yielding, it would be a significant performance hit for a merely cosmetic
/// benefit, as the order of files inside of a folder is not relevant for us.
fn walk<P>(path: P) -> impl Iterator<Item = Result<DirEntry>>
where
    P: AsRef<Path>,
{
    WalkDir::new(path)
        .follow_root_links(false)
        .follow_links(false)
        .into_iter()
        // For now we skip hidden directories to speed up the build, since we
        // do not need to watch icons, but in general we need to find a better
        // method in the future when we integrate large asset directories and
        // libraries that include thousands of icons.
        .filter_entry(|item| {
            !(item.file_type().is_dir()
                && item.file_name().to_str().unwrap_or("").starts_with('.'))
        })
        .map(|item| item.map_err(Into::into))
}

// ----------------------------------------------------------------------------

/// Returns the file identifier for the file or folder at the given path.
#[cfg(target_family = "unix")]
fn get_file_id<P>(path: P) -> io::Result<FileId>
where
    P: AsRef<Path>,
{
    use std::os::unix::fs::MetadataExt;

    // This implementation is taken from the `file-id` crate, but modified to
    // not follow symbolic links, as we track those explicitly
    fs::symlink_metadata(path)
        .map(|metadata| FileId::new_inode(metadata.dev(), metadata.ino()))
}

/// Returns the file identifier for the file or folder at the given path.
#[cfg(target_family = "windows")]
#[inline]
fn get_file_id<P>(path: P) -> io::Result<FileId>
where
    P: AsRef<Path>,
{
    // We should be fine by just using the low resolution variant, as it's much
    // cheaper, and we also don't need to support more than 4b volumes. If it
    // turns out that this breaks in some esoteric cases, we might consider
    // changing this later on, possibly putting it behind a feature flag.
    file_id::get_low_res_file_id(path)
}
