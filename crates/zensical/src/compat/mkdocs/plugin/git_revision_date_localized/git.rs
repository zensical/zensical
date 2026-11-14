// Copyright (c) 2025-2026 Zensical and contributors
// SPDX-License-Identifier: MIT

//! Read-only Git queries and persistent raw history results.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// Commit identity and author timestamp, without a display-dependent format.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Commit {
    pub hash: String,
    pub timestamp: i64,
}

/// Repository facts resolved once for each content rebuild.
#[derive(Debug)]
pub struct Repository {
    pub root: PathBuf,
    pub shallow: bool,
    fingerprint: String,
    head: Option<String>,
    tags: BTreeMap<String, String>,
}

#[derive(Deserialize, Serialize)]
struct Cached {
    fingerprint: String,
    commit: Option<Commit>,
}

/// Git is run without a shell, pager, interactive input, or optional locks.
fn command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(root)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

fn output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = command(root)
        .args(args)
        .output()
        .context("unable to execute git")?;
    if !output.status.success() {
        bail!(
            "git {}: {}",
            args[0],
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn text(root: &Path, args: &[&str]) -> Result<String> {
    let value = String::from_utf8(output(root, args)?)?;
    Ok(value.strip_suffix('\n').unwrap_or(&value).to_owned())
}

impl Repository {
    /// The containing repository is discovered through Git, including worktrees.
    pub fn discover(directory: &Path) -> Result<PathBuf> {
        Ok(PathBuf::from(text(
            directory,
            &["rev-parse", "--show-toplevel"],
        )?))
    }

    /// History inputs and tag names are captured for one rebuild.
    pub fn new(root: PathBuf) -> Result<Self> {
        let head = command(&root)
            .args(["rev-parse", "--verify", "HEAD"])
            .output()?;
        let head = head
            .status
            .success()
            .then(|| String::from_utf8_lossy(&head.stdout).trim().to_owned());
        let shallow_path = text(
            &root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "shallow",
            ],
        )?;
        let shallow = fs::read(shallow_path).unwrap_or_default();
        let grafts_path = text(
            &root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "info/grafts",
            ],
        )?;
        let grafts = fs::read(grafts_path).unwrap_or_default();
        let config = output(&root, &["config", "--null", "--list"])?;
        let replacements = output(
            &root,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/replace",
            ],
        )?;
        // Deepening a shallow clone can change dates without moving HEAD.
        // Effective Git configuration and replacement history are included too.
        let fingerprint = digest(&serde_json::to_vec(&(
            &head,
            &shallow,
            grafts,
            config,
            replacements,
        ))?);
        let refs = text(
            &root,
            &[
                "for-each-ref",
                "--sort=refname",
                "--format=%(objectname)%09%(*objectname)%09%(refname:strip=2)",
                "refs/tags",
            ],
        )?;
        let mut tags = BTreeMap::new();
        for line in refs.lines() {
            if let Some((hash, rest)) = line.split_once('\t')
                && let Some((peeled, tag)) = rest.split_once('\t')
            {
                tags.entry(hash.to_owned())
                    .or_insert_with(|| tag.to_owned());
                if !peeled.is_empty() {
                    tags.entry(peeled.to_owned())
                        .or_insert_with(|| tag.to_owned());
                }
            }
        }
        Ok(Self {
            root,
            shallow: !shallow.is_empty(),
            fingerprint,
            head,
            tags,
        })
    }

    pub fn tag(&self, commit: &Commit) -> String {
        self.tags.get(&commit.hash).cloned().unwrap_or_default()
    }

    /// Only raw commit facts are cached; fallbacks and rendered dates are not.
    pub fn query(
        &self, source: &Path, creation: bool, follow: bool, ignored: &[String],
        cache: &Path,
    ) -> Result<Option<Commit>> {
        let Some(head) = &self.head else {
            return Ok(None);
        };
        let source = source.strip_prefix(&self.root)?;
        let source = if source.as_os_str().is_empty() {
            Path::new(".")
        } else {
            source
        };
        // Creation history is preserved even when revision commits are ignored,
        // as required by the upstream ignored_commits_file option.
        let ignored = if creation { &[][..] } else { ignored };
        let key = serde_json::to_vec(&(
            1_u8, &self.root, source, creation, follow, ignored,
        ))?;
        let path = cache.join(format!("{}.json", digest(&key)));
        if let Ok(data) = fs::read(&path)
            && let Ok(cached) = serde_json::from_slice::<Cached>(&data)
            && cached.fingerprint == self.fingerprint
        {
            return Ok(cached.commit);
        }

        let mut git = command(&self.root);
        // Author timestamps and the exclusion of pure renames are retained
        // from upstream. Creation is the oldest addition in Git's traversal,
        // rather than the minimum timestamp, since author clocks can differ.
        git.args([
            "log",
            "--no-show-signature",
            "--no-decorate",
            "--format=%H %at",
        ]);
        git.arg(if follow { "--follow" } else { "--no-follow" });
        if creation {
            git.arg("--diff-filter=Ar");
        } else {
            git.args([
                "--diff-filter=r",
                "--ignore-all-space",
                "--ignore-blank-lines",
            ])
            .arg(format!("--max-count={}", ignored.len() + 1));
        }
        let result = git.arg(head).arg("--").arg(source).output()?;
        if !result.status.success() {
            bail!(
                "unable to read Git history for {}: {}",
                source.display(),
                String::from_utf8_lossy(&result.stderr).trim()
            );
        }
        let history = String::from_utf8(result.stdout)?;
        let mut commits = Vec::new();
        for line in history.lines().filter(|line| !line.is_empty()) {
            let (hash, timestamp) =
                line.split_once(' ').context("invalid git log output")?;
            commits.push(Commit {
                hash: hash.into(),
                timestamp: timestamp.parse()?,
            });
        }
        let commit = if creation {
            commits.pop()
        } else {
            commits.into_iter().find(|commit| {
                !ignored.iter().any(|prefix| commit.hash.starts_with(prefix))
            })
        };
        let cached = Cached {
            fingerprint: self.fingerprint.clone(),
            commit: commit.clone(),
        };
        // Cache failures do not prevent a build. Atomic replacement prevents
        // partial JSON from being observed by another page or build process.
        if let Ok(data) = serde_json::to_vec(&cached)
            && fs::create_dir_all(cache).is_ok()
        {
            static SEQUENCE: AtomicU64 = AtomicU64::new(0);
            let temporary = path.with_extension(format!(
                "{}.{}.tmp",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            if fs::write(&temporary, data).is_ok() {
                let _ = fs::rename(&temporary, &path);
                let _ = fs::remove_file(temporary);
            }
        }
        Ok(commit)
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
