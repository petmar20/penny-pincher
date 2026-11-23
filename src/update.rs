//! Updates the app from the latest GitHub release.

use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/petmar20/penny-pincher/releases/latest";
const ASSET_NAME: &str = "penny-pincher.exe";

pub struct Release {
    pub version: String,
    download_url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

/// Uses the Windows certificate store, so it works behind proxies that inspect HTTPS.
fn agent() -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent("penny-pincher-updater")
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// The latest release, if it's newer than this build.
pub fn check_for_update() -> Result<Option<Release>> {
    let release: GitHubRelease = agent()
        .get(LATEST_RELEASE_URL)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;

    let latest = parse_version(&release.tag_name).with_context(|| format!("unexpected release tag {}", release.tag_name))?;
    let current = parse_version(env!("CARGO_PKG_VERSION")).expect("the package version is valid");
    if latest <= current {
        return Ok(None);
    }
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name == ASSET_NAME)
        .with_context(|| format!("release {} has no {ASSET_NAME}", release.tag_name))?;
    Ok(Some(Release {
        version: release.tag_name.trim_start_matches('v').to_string(),
        download_url: asset.browser_download_url,
    }))
}

/// Downloads the release and swaps it in for the running exe. It takes effect on the next start.
pub fn install(release: &Release) -> Result<()> {
    let current_exe = std::env::current_exe()?;
    // Next to the exe, so the final swap is a rename on the same drive.
    let download: PathBuf = current_exe.with_extension("download");
    let result = download_and_replace(release, &download);
    let _ = std::fs::remove_file(&download);
    result.context("Couldn't install the update")
}

fn download_and_replace(release: &Release, download: &PathBuf) -> Result<()> {
    let mut response = agent().get(&release.download_url).call()?;
    let mut file = File::create(download)?;
    std::io::copy(&mut response.body_mut().as_reader(), &mut file)?;
    file.sync_all()?;
    drop(file);
    if std::fs::metadata(download)?.len() < 1_000_000 {
        bail!("the downloaded file is too small to be the app");
    }
    self_replace::self_replace(download)?;
    Ok(())
}

/// Starts a fresh copy of the app. The caller then closes this one.
pub fn restart() -> Result<()> {
    std::process::Command::new(std::env::current_exe()?).spawn()?;
    Ok(())
}

/// "v1.2.3" or "1.2.3" as comparable numbers.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.').map(|part| part.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_orders_versions() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert_eq!(parse_version("v1.2"), None);
        assert_eq!(parse_version("v1.2.3-beta"), None);
    }
}
