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

//! Download identities and persistent raw asset caching.

use anyhow::{ensure, Context as _, Result};
use fluent_uri::UriRef;
use percent_encoding::{
    percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::sync::{Arc, OnceLock};
use std::sync::{Condvar, Mutex};
use ureq::tls::{parse_pem, PemItem, TlsConfig};

use crate::path::SitePath;

use super::Instance;

/// A URL's query-sensitive identity, independent of its scheme and fragment.
#[derive(Debug)]
pub(super) struct RemoteUrl {
    pub key: String,
    request: String,
}

/// Raw downloaded bytes and their MIME-resolved output path.
#[derive(Clone, Debug)]
pub(super) struct Downloaded {
    pub path: SitePath,
    pub bytes: Arc<Vec<u8>>,
    pub patched: Arc<OnceLock<Result<Arc<Vec<u8>>, String>>>,
}

/// The effective filename is retained without requiring filesystem symlinks.
#[derive(Deserialize, Serialize)]
struct Metadata {
    path: SitePath,
}

/// Network work is bounded independently of document-rendering concurrency.
#[derive(Debug)]
pub(super) struct Limit {
    maximum: usize,
    active: Mutex<usize>,
    available: Condvar,
}

struct Permit<'a>(&'a Limit);

impl Limit {
    pub fn new(maximum: usize) -> Self {
        Self {
            maximum,
            active: Mutex::new(0),
            available: Condvar::new(),
        }
    }

    fn acquire(&self) -> Result<Permit<'_>> {
        let active = self.active.lock().map_err(|_| {
            anyhow::anyhow!("privacy download limiter was poisoned")
        })?;
        let mut active = self
            .available
            .wait_while(active, |active| *active >= self.maximum)
            .map_err(|_| {
                anyhow::anyhow!("privacy download limiter was poisoned")
            })?;
        *active += 1;
        Ok(Permit(self))
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active.lock() {
            *active -= 1;
            self.0.available.notify_one();
        }
    }
}

impl RemoteUrl {
    pub fn parse(value: &str) -> Result<Self> {
        let uri = parse_url(value).context("invalid external URL")?;
        let authority = uri.authority().context("external URL has no host")?;
        let mut components = Vec::new();
        for component in uri.path().as_str().split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    components.pop();
                }
                component => components.push(component),
            }
        }
        let mut path = components
            .into_iter()
            .map(|component| {
                component.strip_prefix('.').map_or_else(
                    || component.to_owned(),
                    |rest| format!("_{rest}"),
                )
            })
            .collect::<Vec<_>>()
            .join("/");
        if path.is_empty() {
            path = "index".into();
        }
        if let Some(query) =
            uri.query().filter(|query| !query.as_str().is_empty())
        {
            let digest =
                sha1_smol::Sha1::from(query.as_str()).digest().to_string();
            let name = path.rsplit('/').next().unwrap_or(&path);
            let extension = name.rfind('.').map_or(path.len(), |position| {
                path.len() - name.len() + position
            });
            path.insert_str(extension, &format!(".{}", &digest[..8]));
        }
        let key = format!("{authority}/{path}");
        // Percent decoding is required for Material-compatible filenames.
        // Decoded parent components must still be rejected by SitePath.
        percent_decode_str(&key)
            .decode_utf8()?
            .parse::<SitePath>()?;
        let encoded = uri.to_string();
        let request = encoded.split('#').next().unwrap_or(&encoded);
        let request = if request.starts_with("//") {
            format!("http:{request}")
        } else {
            request.to_owned()
        };
        Ok(Self { key, request })
    }
}

/// Explicit CA bundles replace the default roots, as they do in Requests.
pub(super) fn tls_config() -> Result<TlsConfig> {
    let bundle = ["REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE", "SSL_CERT_FILE"]
        .into_iter()
        .find_map(|name| {
            std::env::var_os(name)
                .filter(|path| !path.is_empty())
                .map(|path| (name, std::path::PathBuf::from(path)))
        });
    let Some((name, path)) = bundle else {
        return Ok(TlsConfig::default());
    };
    let pem = fs::read(&path).with_context(|| {
        format!("failed to read {name} CA bundle '{}'", path.display())
    })?;
    let mut certificates = Vec::new();
    for item in parse_pem(&pem) {
        if let PemItem::Certificate(certificate) = item.with_context(|| {
            format!("failed to parse {name} CA bundle '{}'", path.display())
        })? {
            certificates.push(certificate);
        }
    }
    ensure!(
        !certificates.is_empty(),
        "{name} CA bundle '{}' contains no certificates",
        path.display()
    );
    Ok(TlsConfig::builder().root_certs(certificates.into()).build())
}

impl Downloaded {
    pub fn extension(&self) -> String {
        self.path
            .extension()
            .map(|extension| format!(".{extension}"))
            .unwrap_or_default()
    }
}

impl Instance {
    /// Raw cache entries are reused independently of patched output bytes.
    pub(super) fn fetch(&self, remote: &RemoteUrl) -> Result<Downloaded> {
        let identity =
            format!("{}/{}", self.config.assets_fetch_dir, remote.key);
        let digest = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let metadata_path =
            self.cache.join(".metadata").join(format!("{digest}.json"));
        if self.config.cache
            && let Ok(metadata) = fs::read(&metadata_path)
            && let Ok(metadata) = serde_json::from_slice::<Metadata>(&metadata)
            && let Ok(bytes) = fs::read(self.cache.join(metadata.path.as_str()))
        {
            return Ok(Downloaded {
                path: metadata.path,
                bytes: Arc::new(bytes),
                patched: Arc::default(),
            });
        }

        let _permit = self.limit.acquire()?;
        self.report(
            "info",
            &format!("Downloading external file: {}", remote.request),
        )?;
        let mut response = self.agent.get(&remote.request)
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/98.0.4758.102 Safari/537.36")
            .call().context("external asset request failed")?;
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .to_owned();
        let decoded = percent_decode_str(&remote.key).decode_utf8()?;
        let mut path = format!("{}/{decoded}", self.config.assets_fetch_dir);
        let extension = match mime.as_str() {
            "application/javascript" | "text/javascript" => ".js",
            "text/css" => ".css",
            "image/avif" => ".avif",
            "image/gif" => ".gif",
            "image/jpeg" => ".jpg",
            "image/png" => ".png",
            "image/svg+xml" => ".svg",
            "image/webp" => ".webp",
            _ => "",
        };
        if !extension.is_empty() && !path.ends_with(extension) {
            path.push_str(extension);
        }
        let path = path.parse::<SitePath>()?;
        // The HTTP client's default body cap is unsuitable for media files.
        let bytes = response
            .body_mut()
            .with_config()
            .limit(512 * 1024 * 1024)
            .read_to_vec()?;
        let target = self.cache.join(path.as_str());
        fs::create_dir_all(
            target.parent().context("asset cache path has no parent")?,
        )?;
        fs::create_dir_all(metadata_path.parent().unwrap())?;
        atomic_write(&target, &bytes)?;
        atomic_write(
            &metadata_path,
            &serde_json::to_vec(&Metadata { path: path.clone() })?,
        )?;
        Ok(Downloaded {
            path,
            bytes: Arc::new(bytes),
            patched: Arc::default(),
        })
    }
}

/// Writes are made atomic across instances and concurrent builds.
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(
        ".privacy-{}-{sequence}.tmp",
        std::process::id()
    ));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

/// Filesystem spelling is escaped for HTML and CSS URL references.
pub(super) fn public_path(path: &SitePath) -> String {
    const ESCAPE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'\'')
        .add(b'<')
        .add(b'>')
        .add(b'`')
        .add(b'#')
        .add(b'?')
        .add(b'%')
        .add(b'(')
        .add(b')');
    utf8_percent_encode(path.as_str(), ESCAPE).to_string()
}

/// Unescaped Unicode and spaces are accepted by Material through requests.
pub(super) fn parse_url(value: &str) -> Option<UriRef<String>> {
    const ESCAPE: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'<')
        .add(b'>')
        .add(b'`')
        .add(b'\\')
        .add(b'{')
        .add(b'}')
        .add(b'^')
        .add(b'|');
    UriRef::parse(utf8_percent_encode(value, ESCAPE).to_string()).ok()
}

#[cfg(test)]
mod tests {
    use super::RemoteUrl;

    #[test]
    fn queries_are_hashed_and_fragments_share_a_download() {
        let light = RemoteUrl::parse(
            "https://example.com/a/../.hidden/image.svg?size=2#light",
        )
        .unwrap();
        let dark = RemoteUrl::parse(
            "http://example.com/.hidden/image.svg?size=2#dark",
        )
        .unwrap();

        assert_eq!(light.key, dark.key);
        assert_eq!(
            light.key,
            format!(
                "example.com/_hidden/image.{}.svg",
                &sha1_smol::Sha1::from("size=2").digest().to_string()[..8]
            )
        );
    }

    #[test]
    fn decoded_traversal_is_rejected() {
        assert!(RemoteUrl::parse("https://example.com/%2e%2e/secret").is_err());
        assert!(RemoteUrl::parse("https://example.com/a%5cb").is_err());
    }
}
