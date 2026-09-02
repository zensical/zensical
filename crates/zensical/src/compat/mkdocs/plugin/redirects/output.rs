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

//! Redirect output.

use anyhow::{bail, Result};
use std::fs;

use crate::path::OutputRoot;

use super::plan::Snapshot;

/// Redirect document emitted by mkdocs-redirects 1.2.2.
const HTML_TEMPLATE: &str = r##"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="{url}">
    <script>var anchor=window.location.hash.substr(1);location.href="{url}"+(anchor?"#"+anchor:"")</script>
    <meta http-equiv="refresh" content="0; url={url}">
</head>
<body>
You're being redirected to a <a href="{url}">new destination</a>.
</body>
</html>
"##;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Reconciles one complete redirect snapshot with the site directory.
///
/// Missing targets retract outputs created by earlier revisions. Warnings are
/// reported after all files have been reconciled so strict mode cannot leave a
/// stale redirect behind merely because its target disappeared.
pub fn write(
    output: &OutputRoot, snapshot: &Snapshot, strict: bool,
) -> Result<()> {
    for redirect in &snapshot.redirects {
        let path = output.join(&redirect.output);
        if let Some(target) = &redirect.target {
            fs::create_dir_all(path.parent().expect("redirect has parent"))?;
            fs::write(path, render(target))?;
        } else if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    for warning in &snapshot.warnings {
        eprintln!("WARNING -  {warning}");
    }
    if strict && !snapshot.warnings.is_empty() {
        bail!("Aborted because --strict flag is set")
    }
    Ok(())
}

/// Renders the upstream-compatible redirect document.
fn render(target: &str) -> String {
    HTML_TEMPLATE.replace("{url}", target)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{render, write};
    use crate::compat::mkdocs::plugin::redirects::plan::{Redirect, Snapshot};
    use crate::path::{OutputRoot, SitePath};

    #[test]
    fn renders_upstream_document() {
        let html = render("../new/");
        assert_eq!(
            html,
            r##"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="../new/">
    <script>var anchor=window.location.hash.substr(1);location.href="../new/"+(anchor?"#"+anchor:"")</script>
    <meta http-equiv="refresh" content="0; url=../new/">
</head>
<body>
You're being redirected to a <a href="../new/">new destination</a>.
</body>
</html>
"##
        );
    }

    #[test]
    fn removes_a_stale_redirect_when_its_target_disappears() {
        let directory = tempfile::tempdir().unwrap();
        let output = "old/index.html".parse::<SitePath>().unwrap();
        let valid = Snapshot {
            redirects: vec![Redirect {
                output: output.clone(),
                target: Some("../new/".into()),
            }],
            warnings: Vec::new(),
        };
        let root = OutputRoot::prepare(directory.path()).unwrap();
        write(&root, &valid, false).unwrap();
        assert!(directory.path().join(output.as_str()).is_file());

        let missing = Snapshot {
            redirects: vec![Redirect {
                output: output.clone(),
                target: None,
            }],
            warnings: vec!["missing".into()],
        };
        write(&root, &missing, false).unwrap();
        assert!(!directory.path().join(output.as_str()).exists());
        assert!(write(&root, &missing, true).is_err());
    }
}
