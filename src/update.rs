use anyhow::{Context, Result, bail};
use flate2::read::GzDecoder;
use fs2::FileExt;
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tar::Archive;

const APP_NAME: &str = "gallendar";
const DEFAULT_REPO: &str = "RoTorEx/gallendar";
const AUTO_UPDATE_INTERVAL_SECONDS: u64 = 24 * 60 * 60;
const MACOS_AARCH64_ASSET: &str = "gallendar-macos-aarch64.tar.gz";
const MACOS_X86_64_ASSET: &str = "gallendar-macos-x86_64.tar.gz";
const LINUX_X86_64_ASSET: &str = "gallendar-linux-x86_64.tar.gz";

// The automatic worker is an intended product process, not a CLI task dependency.
// It is isolated from command output and never runs for local development binaries.
pub fn start_automatic() {
    let _ = (|| -> Result<()> {
        let home = crate::home()?;
        let executable = env::current_exe()?;
        if executable != home.join("bin/gallendar") || !automatic_due(&home, now_seconds()?) {
            return Ok(());
        }
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(home.join("update.log"))?;
        let mut worker = Command::new(executable)
            .args(["update", "--automatic"])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        std::thread::spawn(move || {
            let _ = worker.wait();
        });
        Ok(())
    })();
}

pub fn run(repo: Option<String>, automatic: bool) -> Result<()> {
    let executable = env::current_exe()?;
    let parent = executable.parent().context("Executable has no parent")?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(parent.join(".gallendar-update.lock"))?;
    if lock.try_lock_exclusive().is_err() {
        if automatic {
            return Ok(());
        }
        bail!("Another gallendar update is already running");
    }
    if automatic {
        let home = crate::home()?;
        if executable != home.join("bin/gallendar") {
            return Ok(());
        }
        let now = now_seconds()?;
        if !automatic_due(&home, now) {
            return Ok(());
        }
        let mut stamp = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(home.join("update-last-check"))?;
        writeln!(stamp, "{now}")?;
    }
    run_locked(repo, automatic)
}

fn now_seconds() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

fn automatic_due(home: &Path, now: u64) -> bool {
    let last = fs::read_to_string(home.join("update-last-check"))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok());
    last.is_none_or(|last| now.saturating_sub(last) >= AUTO_UPDATE_INTERVAL_SECONDS)
}

fn run_locked(repo: Option<String>, automatic: bool) -> Result<()> {
    let repo = repo
        .or_else(|| env::var("GALLENDAR_RELEASE_REPO").ok())
        .or_else(|| option_env!("GALLENDAR_RELEASE_REPO").map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_REPO.to_owned());
    validate_repo(&repo)?;
    let release_base_url = format!("https://github.com/{repo}/releases/latest/download");
    let asset = release_asset_for(env::consts::OS, env::consts::ARCH).ok_or_else(|| {
        anyhow::anyhow!(
            "gallendar update currently supports Apple Silicon / Intel macOS and x86_64 Linux"
        )
    })?;
    let current_exe =
        env::current_exe().context("cannot locate the current gallendar executable")?;
    let temp_dir = UpdateTempDir::create()?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    if automatic {
        // The public redirect avoids unauthenticated API rate limits and needs no token.
        let response = client
            .head(format!("https://github.com/{repo}/releases/latest"))
            .timeout(std::time::Duration::from_secs(10))
            .send()?
            .error_for_status()?;
        let latest = version_from_release_url(response.url(), &repo)?;
        let current = version_number(&format!("gallendar {}", env!("CARGO_PKG_VERSION")))?;
        if latest <= current {
            return Ok(());
        }
    }

    let archive_url = format!("{release_base_url}/{asset}");
    let checksum_url = format!("{archive_url}.sha256");
    eprintln!("Downloading {archive_url}");

    let archive_bytes = download_bytes(&client, &archive_url)?;
    let checksum_bytes = download_bytes(&client, &checksum_url)?;
    let checksum_text =
        std::str::from_utf8(&checksum_bytes).context("release checksum is not valid UTF-8")?;
    verify_checksum(&archive_bytes, checksum_text)?;

    let updated = temp_dir.path().join(APP_NAME);
    extract_binary(&archive_bytes, &updated)?;
    set_executable(&updated)?;

    let updated_version = read_version(&updated)?;
    let current_version = format!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
    if updated_version == current_version {
        println!("Already up to date: {updated_version}");
        return Ok(());
    }

    if version_number(&updated_version)? < version_number(&current_version)? {
        bail!("Latest release is older than this binary; refusing downgrade");
    }
    install_binary(&updated, &current_exe)?;
    println!("Updated {current_version} -> {updated_version}");
    Ok(())
}

fn release_asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some(MACOS_AARCH64_ASSET),
        ("macos", "x86_64") => Some(MACOS_X86_64_ASSET),
        ("linux", "x86_64") => Some(LINUX_X86_64_ASSET),
        _ => None,
    }
}

fn download_bytes(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("release download failed for {url}"))?;

    Ok(response
        .bytes()
        .with_context(|| format!("failed to read {url}"))?
        .to_vec())
}

fn verify_checksum(archive: &[u8], checksum_file: &str) -> Result<()> {
    let expected = parse_checksum(checksum_file)?;
    let actual = format!("{:x}", Sha256::digest(archive));
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("release archive checksum mismatch");
    }
    Ok(())
}

fn parse_checksum(checksum_file: &str) -> Result<&str> {
    let checksum = checksum_file
        .split_whitespace()
        .next()
        .context("release checksum file is empty")?;
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("release checksum is not a SHA-256 digest");
    }
    Ok(checksum)
}

fn extract_binary(archive_bytes: &[u8], destination: &Path) -> Result<()> {
    let decoder = GzDecoder::new(Cursor::new(archive_bytes));
    let mut archive = Archive::new(decoder);
    let mut found = false;

    for entry in archive.entries().context("cannot read release archive")? {
        let mut entry = entry.context("cannot read release archive entry")?;
        if entry
            .path()
            .context("release archive path is invalid")?
            .as_ref()
            != Path::new(APP_NAME)
        {
            continue;
        }
        if found || !entry.header().entry_type().is_file() {
            bail!("release archive contains an invalid gallendar entry");
        }
        entry
            .unpack(destination)
            .context("cannot extract updated gallendar binary")?;
        found = true;
    }

    if !found {
        bail!("release archive does not contain a gallendar binary");
    }
    Ok(())
}

fn read_version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("-V")
        .stdin(Stdio::null())
        .output()
        .context("cannot run the downloaded gallendar binary")?;
    if !output.status.success() {
        bail!("downloaded gallendar binary failed its version check");
    }
    let version = std::str::from_utf8(&output.stdout)
        .context("downloaded gallendar version is not valid UTF-8")?
        .trim();
    if !valid_version_output(version) {
        bail!("downloaded binary returned an invalid gallendar version");
    }
    Ok(version.to_owned())
}

fn valid_version_output(version: &str) -> bool {
    version_number(version).is_ok()
}

fn version_from_release_url(url: &reqwest::Url, repo: &str) -> Result<[u64; 3]> {
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        bail!("Latest release redirected outside GitHub");
    }
    let (repository, tag) = url
        .path()
        .rsplit_once("/releases/tag/")
        .context("Latest release did not redirect to a version tag")?;
    if !repository.eq_ignore_ascii_case(&format!("/{repo}")) {
        bail!("Latest release redirected to another repository");
    }
    let version = tag
        .strip_prefix('v')
        .context("Release tag must start with v")?;
    version_number(&format!("gallendar {version}"))
}

fn version_number(text: &str) -> Result<[u64; 3]> {
    let number = text
        .strip_prefix("gallendar ")
        .context("Invalid binary name in version")?;
    let parts = number
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                bail!("Invalid release version");
            }
            Ok(p.parse::<u64>()?)
        })
        .collect::<Result<Vec<_>>>()?;
    parts
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid release version"))
}

fn install_binary(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .context("current gallendar executable has no parent directory")?;
    let temporary = parent.join(format!(".gallendar-update-{}", std::process::id()));

    let result = (|| -> Result<()> {
        fs::copy(source, &temporary)
            .with_context(|| format!("cannot copy updated gallendar to {}", temporary.display()))?;
        set_executable(&temporary)?;
        fs::rename(&temporary, destination)
            .with_context(|| format!("cannot replace {}", destination.display()))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn set_executable(path: &Path) -> Result<()> {
    let mut permissions = fs::metadata(path)
        .with_context(|| format!("cannot inspect {}", path.display()))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)
        .with_context(|| format!("cannot make {} executable", path.display()))
}

fn validate_repo(repo: &str) -> Result<()> {
    let parts: Vec<_> = repo.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == "."
                || *p == ".."
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        bail!("Expected GitHub repository OWNER/REPO");
    }
    Ok(())
}

struct UpdateTempDir {
    path: PathBuf,
}

impl UpdateTempDir {
    fn create() -> Result<Self> {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("system time is before the Unix epoch")?
            .as_nanos();
        let path = env::temp_dir().join(format!("gallendar-update-{suffix}"));
        fs::create_dir(&path)
            .with_context(|| format!("cannot create update directory {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for UpdateTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_every_published_macos_asset() {
        assert_eq!(
            release_asset_for("macos", "aarch64"),
            Some("gallendar-macos-aarch64.tar.gz")
        );
        assert_eq!(
            release_asset_for("macos", "x86_64"),
            Some("gallendar-macos-x86_64.tar.gz")
        );
        assert_eq!(
            release_asset_for("linux", "x86_64"),
            Some(LINUX_X86_64_ASSET)
        );
    }

    #[test]
    fn accepts_matching_sha256_checksum() {
        verify_checksum(
            b"gallendar release",
            "27f860b0c532675a4f58f463bfa283329956d022d243c16ba82a74a946e1d802  gallendar.tar.gz",
        )
        .unwrap();
    }

    #[test]
    fn rejects_mismatched_sha256_checksum() {
        let error = verify_checksum(
            b"tampered",
            "27f860b0c532675a4f58f463bfa283329956d022d243c16ba82a74a946e1d802  gallendar.tar.gz",
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "release archive checksum mismatch");
    }

    #[test]
    fn rejects_malformed_checksum() {
        let error = parse_checksum("not-a-checksum gallendar.tar.gz").unwrap_err();

        assert_eq!(
            error.to_string(),
            "release checksum is not a SHA-256 digest"
        );
    }

    #[test]
    fn validates_downloaded_version_shape() {
        assert!(valid_version_output("gallendar 0.1.0"));
        assert!(!valid_version_output("other 0.1.0"));
        assert!(!valid_version_output("gallendar 0.1.0 extra"));
    }
    #[test]
    fn automatic_checks_are_daily_and_clock_rollback_does_not_trigger_them() {
        let temp = UpdateTempDir::create().unwrap();
        assert!(automatic_due(temp.path(), 100));
        fs::write(temp.path().join("update-last-check"), "100").unwrap();
        assert!(!automatic_due(temp.path(), 99));
        assert!(!automatic_due(
            temp.path(),
            100 + AUTO_UPDATE_INTERVAL_SECONDS - 1
        ));
        assert!(automatic_due(
            temp.path(),
            100 + AUTO_UPDATE_INTERVAL_SECONDS
        ));
        fs::write(temp.path().join("update-last-check"), "corrupt").unwrap();
        assert!(automatic_due(temp.path(), 100));
    }

    #[test]
    fn version_comparison_is_numeric_and_rejects_overflow() {
        assert!(
            version_number("gallendar 0.10.0").unwrap()
                > version_number("gallendar 0.9.9").unwrap()
        );
        assert!(version_number("gallendar 0.0.9999999999999999999999999").is_err());
    }

    #[test]
    fn replacement_preserves_old_binary_on_failure_and_cleans_staging_file() {
        let temp = UpdateTempDir::create().unwrap();
        let destination = temp.path().join(APP_NAME);
        let source = temp.path().join("downloaded");
        fs::write(&destination, b"old").unwrap();
        assert!(install_binary(&source, &destination).is_err());
        assert_eq!(fs::read(&destination).unwrap(), b"old");
        fs::write(&source, b"new").unwrap();
        install_binary(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[test]
    fn extraction_rejects_duplicate_or_symlink_binary_and_ignores_config() {
        fn archive(entries: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
            let gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            let mut builder = tar::Builder::new(gzip);
            for (name, kind, content) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o755);
                header.set_entry_type(*kind);
                if kind.is_symlink() {
                    header.set_link_name("outside").unwrap();
                }
                header.set_cksum();
                builder.append_data(&mut header, name, *content).unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap()
        }
        let temp = UpdateTempDir::create().unwrap();
        let destination = temp.path().join("new-binary");
        let bytes = archive(&[
            ("config.toml", tar::EntryType::Regular, b"unwanted"),
            (APP_NAME, tar::EntryType::Regular, b"replacement"),
        ]);
        extract_binary(&bytes, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"replacement");
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
        let duplicate = archive(&[
            (APP_NAME, tar::EntryType::Regular, b"first"),
            (APP_NAME, tar::EntryType::Regular, b"second"),
        ]);
        assert!(extract_binary(&duplicate, &destination).is_err());
        let symlink = archive(&[(APP_NAME, tar::EntryType::Symlink, b"")]);
        assert!(extract_binary(&symlink, &destination).is_err());
    }
    #[test]
    fn public_latest_redirect_validates_repository_and_version() {
        let repo = "RoTorEx/gallendar";
        let url = reqwest::Url::parse("https://github.com/RoTorEx/gallendar/releases/tag/v0.1.1")
            .unwrap();
        assert_eq!(version_from_release_url(&url, repo).unwrap(), [0, 1, 1]);
        assert_eq!(
            version_from_release_url(&url, "rotorex/gallendar").unwrap(),
            [0, 1, 1]
        );
        for text in [
            "https://github.com/RoTorEx/gallendar/releases/latest",
            "https://github.com/other/gallendar/releases/tag/v0.1.1",
            "https://example.com/RoTorEx/gallendar/releases/tag/v0.1.1",
            "https://github.com/RoTorEx/gallendar/releases/tag/beta",
            "https://github.com/RoTorEx/gallendar/releases/tag/v0.1.1-extra",
        ] {
            assert!(version_from_release_url(&reqwest::Url::parse(text).unwrap(), repo).is_err());
        }
    }
}
