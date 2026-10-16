// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The latest nightly: Suhina's release workflow keeps it under the tag `nightly`, replaced twice a
//! day, with one package per platform (`suhina-nightly-<platform>.<ext>`) and a `.sha256` beside
//! each. The download links stay the same from one nightly to the next.

use serde_json::Value;

/// The release tag the nightly is published under.
pub const TAG: &str = "nightly";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nightly {
    /// `0.0.1-nightly.20261016+7e0d477`, from the release name.
    pub version: String,
    /// `2026-10-16`, the day it was published.
    pub date: String,
    pub page: String,
    pub downloads: Vec<Download>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub platform: &'static str,
    pub url: String,
    pub size_bytes: u64,
}

/// Platforms in the order they are shown, by the word in the package name.
const PLATFORMS: [(&str, &str); 3] = [
    ("windows", "Windows"),
    ("macos", "macOS"),
    ("linux", "Linux"),
];

impl Nightly {
    /// Reads GitHub's answer to `GET /repos/{owner}/{repo}/releases/tags/nightly`. Checksums and
    /// packages of unknown platforms are left out; `None` when the answer has no packages.
    pub fn from_release(release: &Value) -> Option<Self> {
        let name = release.get("name")?.as_str()?;
        let version = name.rsplit(' ').next().unwrap_or(name).to_owned();
        let date = release
            .get("published_at")
            .and_then(Value::as_str)
            .and_then(|at| at.get(..10))
            .unwrap_or_default()
            .to_owned();
        let page = release.get("html_url")?.as_str()?.to_owned();
        let assets = release.get("assets")?.as_array()?;

        let downloads: Vec<Download> = PLATFORMS
            .iter()
            .filter_map(|(word, platform)| {
                assets.iter().find_map(|asset| {
                    let name = asset.get("name")?.as_str()?;
                    if name.ends_with(".sha256") || !name.contains(word) {
                        return None;
                    }
                    Some(Download {
                        platform,
                        url: asset.get("browser_download_url")?.as_str()?.to_owned(),
                        size_bytes: asset.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
            })
            .collect();
        (!downloads.is_empty()).then_some(Nightly {
            version,
            date,
            page,
            downloads,
        })
    }
}

/// `23.3 MB`.
pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release() -> Value {
        let url = "https://github.com/suhina-daw/suhina-daw/releases/download/nightly";
        json!({
            "name": "Suhina nightly 0.0.1-nightly.20261016+7e0d477",
            "tag_name": "nightly",
            "html_url": "https://github.com/suhina-daw/suhina-daw/releases/tag/nightly",
            "published_at": "2026-10-16T18:16:45Z",
            "assets": [
                { "name": "suhina-nightly-linux-x86_64.tar.gz", "size": 23299611, "browser_download_url": format!("{url}/suhina-nightly-linux-x86_64.tar.gz") },
                { "name": "suhina-nightly-linux-x86_64.tar.gz.sha256", "size": 101, "browser_download_url": format!("{url}/suhina-nightly-linux-x86_64.tar.gz.sha256") },
                { "name": "suhina-nightly-macos-universal.tar.gz", "size": 34363292, "browser_download_url": format!("{url}/suhina-nightly-macos-universal.tar.gz") },
                { "name": "suhina-nightly-windows-x86_64.zip.sha256", "size": 100, "browser_download_url": format!("{url}/suhina-nightly-windows-x86_64.zip.sha256") },
                { "name": "suhina-nightly-windows-x86_64.zip", "size": 17926253, "browser_download_url": format!("{url}/suhina-nightly-windows-x86_64.zip") },
            ],
        })
    }

    #[test]
    fn reads_the_nightly_release() {
        let nightly = Nightly::from_release(&release()).unwrap();
        assert_eq!(nightly.version, "0.0.1-nightly.20261016+7e0d477");
        assert_eq!(nightly.date, "2026-10-16");
        let platforms: Vec<_> = nightly.downloads.iter().map(|d| d.platform).collect();
        assert_eq!(platforms, ["Windows", "macOS", "Linux"]);
        assert!(
            nightly.downloads[0]
                .url
                .ends_with("suhina-nightly-windows-x86_64.zip")
        );
        assert_eq!(megabytes(nightly.downloads[0].size_bytes), "17.9 MB");
    }

    #[test]
    fn a_release_without_packages_is_none() {
        let mut empty = release();
        empty["assets"] = json!([]);
        assert_eq!(Nightly::from_release(&empty), None);
        assert_eq!(
            Nightly::from_release(&json!({ "message": "Not Found" })),
            None
        );
    }
}
