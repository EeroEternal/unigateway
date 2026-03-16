use anyhow::{Context, Result, bail};
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde_json::Value;
use std::env;
use std::time::Duration;

const REPO: &str = "EeroEternal/unigateway";
const BIN: &str = "ug";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

fn detect_target() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        (os, arch) => bail!("Unsupported platform: {os}/{arch}"),
    }
}

pub async fn run_upgrade() -> Result<()> {
    println!("Current version: {}", style(CURRENT_VERSION).cyan());
    println!("Checking for updates...");

    let client = Client::new();
    let release: Value = client
        .get(format!(
            "https://api.github.com/repos/{REPO}/releases/latest"
        ))
        .header("User-Agent", "ug-upgrade")
        .send()
        .await?
        .error_for_status()
        .context("failed to fetch latest release")?
        .json()
        .await?;

    let latest_tag = release["tag_name"]
        .as_str()
        .context("missing tag_name in release")?;
    let latest_version = latest_tag.trim_start_matches('v');

    if latest_version == CURRENT_VERSION {
        println!("{}", style("Already up to date.").green());
        return Ok(());
    }

    println!(
        "{} New version available: {} -> {}",
        style("✨").yellow(),
        style(CURRENT_VERSION).dim(),
        style(latest_version).green().bold()
    );

    let target = detect_target()?;
    let archive_name = format!("{BIN}-{target}.tar.gz");
    let download_url =
        format!("https://github.com/{REPO}/releases/download/{latest_tag}/{archive_name}");

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} Downloading {msg}")
            .unwrap(),
    );
    pb.set_message(archive_name.clone());
    pb.enable_steady_tick(Duration::from_millis(100));

    let response = client
        .get(&download_url)
        .send()
        .await?
        .error_for_status()
        .context("failed to download release (binary may not be ready yet)")?;

    let bytes = response.bytes().await?;
    pb.finish_with_message("Download complete");

    let tmpdir = tempfile::tempdir().context("create temp dir")?;
    let archive_path = tmpdir.path().join(&archive_name);
    std::fs::write(&archive_path, &bytes)?;

    println!("Extracting...");
    let status = std::process::Command::new("tar")
        .args([
            "xzf",
            &archive_path.to_string_lossy(),
            "-C",
            &tmpdir.path().to_string_lossy(),
        ])
        .status()
        .context("tar failed")?;
    if !status.success() {
        bail!("tar extraction failed");
    }

    let new_bin = tmpdir.path().join(BIN);
    let dest = env::current_exe().context("cannot determine current executable")?;
    let dest_dir = dest.parent().context("cannot determine install dir")?;

    let writable = std::fs::metadata(dest_dir)
        .map(|_| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let meta = std::fs::metadata(dest_dir).unwrap();
                meta.uid() == unsafe { libc::getuid() } || unsafe { libc::getuid() } == 0
            }
            #[cfg(not(unix))]
            true
        })
        .unwrap_or(false);

    if writable {
        std::fs::copy(&new_bin, &dest)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;
        }
    } else {
        println!(
            "{} Need sudo to install to {}",
            style("🔑").yellow(),
            style(dest.display()).cyan()
        );
        let s = std::process::Command::new("sudo")
            .args(["cp", &new_bin.to_string_lossy(), &dest.to_string_lossy()])
            .status()
            .context("sudo cp failed")?;
        if !s.success() {
            bail!("sudo cp failed with exit code: {s}");
        }
    }

    println!(
        "{} Upgraded to {}",
        style("🚀").green(),
        style(latest_version).bold()
    );
    Ok(())
}
