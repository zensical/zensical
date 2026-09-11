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
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};

use crate::path::OutputRoot;

use super::plan::Snapshot;

/// Shared shell for physical redirect documents.
const HTML_TEMPLATE: &str = r#"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="{url}">
    <script>{script}</script>
    <meta http-equiv="refresh" content="0; url={url}">
</head>
<body>
You're being redirected to a <a href="{url}">new destination</a>.
</body>
</html>
"#;

/// Redirect script emitted by mkdocs-redirects 1.2.2.
const SCRIPT: &str = r##"var anchor=window.location.hash.substr(1);location.href="{url}"+(anchor?"#"+anchor:"")"##;

/// Redirect script that checks configured fragment overrides before falling
/// back to the whole-page target and preserving the original fragment.
const SCRIPT_WITH_FRAGMENTS: &str = r#"var anchor=window.location.hash,redirects={redirects},target;for(var source in redirects)if(new URL(source,location.href).hash===anchor){target=redirects[source];break}location.href=target||"{url}"+anchor"#;

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
            fs::write(path, render(target, &redirect.overrides))?;
        } else if path.is_file() {
            fs::remove_file(path)?;
        }
    }
    if let Some(manifest) = &snapshot.manifest {
        // Fragments never reach the server, so anchor redirects are written
        // into one manifest for the browser integration to resolve.
        let path =
            output.join(&"redirect.json".parse().expect("static site path"));
        fs::create_dir_all(path.parent().expect("invariant"))?;
        let mut writer = BufWriter::new(fs::File::create(path)?);
        serde_json::to_writer(&mut writer, manifest)?;
        writer.flush()?;
    }
    for warning in &snapshot.warnings {
        eprintln!("WARNING -  {warning}");
    }
    if strict && !snapshot.warnings.is_empty() {
        bail!("Aborted because --strict flag is set")
    }
    Ok(())
}

/// Renders one physical redirect document.
///
/// Redirects without fragment overrides retain the upstream script verbatim.
/// When overrides exist, JSON supplies only the fragments belonging to this
/// page. Escaping closing tags keeps that JSON inside the script.
fn render(target: &str, overrides: &BTreeMap<String, String>) -> String {
    let script = if overrides.is_empty() {
        SCRIPT.replace("{url}", target)
    } else {
        let overrides = serde_json::to_string(overrides)
            .expect("redirect fragments are strings")
            .replace("</", "<\\/");
        SCRIPT_WITH_FRAGMENTS
            .replace("{url}", target)
            .replace("{redirects}", &overrides)
    };
    HTML_TEMPLATE
        .replace("{url}", target)
        .replace("{script}", &script)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{render, write};
    use crate::compat::mkdocs::plugin::redirects::plan::{Redirect, Snapshot};
    use crate::path::{OutputRoot, SitePath};
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn renders_upstream_document() {
        let html = render("../new/", &BTreeMap::new());
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
    fn renders_fragment_specific_destinations() {
        let html = render(
            "../new/",
            &BTreeMap::from([(
                "#install".into(),
                "../guides/install/#linux".into(),
            )]),
        );
        assert_eq!(
            html,
            r##"
<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>Redirecting...</title>
    <link rel="canonical" href="../new/">
    <script>var anchor=window.location.hash,redirects={"#install":"../guides/install/#linux"},target;for(var source in redirects)if(new URL(source,location.href).hash===anchor){target=redirects[source];break}location.href=target||"../new/"+anchor</script>
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
                overrides: BTreeMap::new(),
            }],
            manifest: Some(BTreeMap::new()),
            warnings: Vec::new(),
        };
        let root = OutputRoot::prepare(directory.path()).unwrap();
        write(&root, &valid, false).unwrap();
        assert!(directory.path().join(output.as_str()).is_file());

        let missing = Snapshot {
            redirects: vec![Redirect {
                output: output.clone(),
                target: None,
                overrides: BTreeMap::new(),
            }],
            manifest: Some(BTreeMap::new()),
            warnings: vec!["missing".into()],
        };
        write(&root, &missing, false).unwrap();
        assert!(!directory.path().join(output.as_str()).exists());
        assert!(write(&root, &missing, true).is_err());
    }

    #[test]
    fn writes_anchor_redirect_manifest() {
        let directory = tempfile::tempdir().unwrap();
        let root = OutputRoot::prepare(directory.path()).unwrap();
        let snapshot = Snapshot {
            redirects: Vec::new(),
            manifest: Some(BTreeMap::from([(
                "guide/#old".into(),
                "reference/#new".into(),
            )])),
            warnings: Vec::new(),
        };

        write(&root, &snapshot, false).unwrap();
        assert_eq!(
            fs::read_to_string(directory.path().join("redirect.json")).unwrap(),
            r#"{"guide/#old":"reference/#new"}"#
        );
    }
}
