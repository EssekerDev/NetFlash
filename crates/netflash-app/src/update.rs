//! GitHub Releases check + unsigned .app replace. Not telemetry.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;

const REPO: &str = "EssekerDev/NetFlash";
const USER_AGENT: &str = concat!("NetFlash/", env!("CARGO_PKG_VERSION"));
const ZIP_PREFIX: &str = "https://github.com/EssekerDev/NetFlash/releases/download/";

/// A newer GitHub release we can install.
#[derive(Debug, Clone)]
pub struct Latest {
    /// Display version without a leading `v`.
    pub version: String,
    /// Direct zip URL (`NetFlash-*-macos.zip`).
    pub zip_url: String,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// `v1.2.3` / `1.2.3` → `(1,2,3)`. Extra suffixes are ignored after the patch.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches('v').trim_start_matches('V');
    let mut parts = s.split('.');
    let major = parts.next()?.split('-').next()?.parse().ok()?;
    let minor = parts.next()?.split('-').next()?.parse().ok()?;
    let patch = parts.next()?.split('-').next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// True when `remote` is a strictly newer semver than `local`.
pub fn is_newer(remote: &str, local: &str) -> bool {
    match (parse_version(remote), parse_version(local)) {
        (Some(r), Some(l)) => r > l,
        _ => false,
    }
}

/// `Some` iff this process is running from `Something.app/Contents/MacOS/…`.
pub fn current_app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name()?.to_str()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()?.to_str()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.file_name()?.to_str()? != "NetFlash.app" {
        return None;
    }
    Some(app.to_path_buf())
}

/// One-shot GitHub poll. `Ok(None)` = up to date or no macOS zip.
pub fn fetch_latest(local_version: &str) -> Result<Option<Latest>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let release: GhRelease = rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|e| e.to_string())?;
        let res = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("github {}", res.status()));
        }
        res.json().await.map_err(|e| e.to_string())
    })?;
    if !is_newer(&release.tag_name, local_version) {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.starts_with("NetFlash-") && a.name.ends_with("-macos.zip"));
    let Some(asset) = asset else {
        return Ok(None);
    };
    if !is_trusted_zip_url(&asset.browser_download_url) {
        return Ok(None);
    }
    let version = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_owned();
    Ok(Some(Latest {
        version,
        zip_url: asset.browser_download_url.clone(),
    }))
}

/// Download the zip, unpack `NetFlash.app`, spawn a replace helper, then the
/// caller must exit so `ditto` can overwrite the running bundle.
pub fn stage_replace(zip_url: &str, dest_app: &Path) -> Result<(), String> {
    if !is_trusted_zip_url(zip_url) {
        return Err("untrusted update url".into());
    }
    if dest_app.file_name().and_then(|n| n.to_str()) != Some("NetFlash.app") {
        return Err("destination is not NetFlash.app".into());
    }
    let pid = std::process::id();
    let work = std::env::temp_dir().join(format!("netflash-update-{pid}"));
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let zip_path = work.join("update.zip");
    let extract = work.join("extract");
    fs::create_dir_all(&extract).map_err(|e| e.to_string())?;

    let rt = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(async {
        let client = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let bytes = client
            .get(zip_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        fs::write(&zip_path, &bytes).map_err(|e| e.to_string())
    })?;

    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(&zip_path)
        .arg(&extract)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("unzip failed".into());
    }

    let staged = find_app(&extract).ok_or_else(|| "zip has no NetFlash.app".to_owned())?;
    if !is_under(&staged, &extract) {
        return Err("extracted app escaped the staging directory".into());
    }

    // Detach the helper: the tray process exits so the bundle can be replaced.
    let script_path = std::env::temp_dir().join(format!("netflash-apply-{pid}.sh"));
    let helper = r#"#!/bin/bash
set -e
log="${TMPDIR:-/tmp}/netflash-update.log"
exec >>"$log" 2>&1
echo "$(date) wait pid=$1 src=$2 dst=$3"
pid="$1"
src="$2"
dst="$3"
work="$4"
script="$0"
while /bin/kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
sleep 1
echo "$(date) replace $src -> $dst"
incoming="${dst}.incoming"
rm -rf "$incoming"
/usr/bin/ditto "$src" "$incoming"
/usr/bin/xattr -cr "$incoming" 2>/dev/null || true
/usr/bin/codesign --force --deep --sign - "$incoming" 2>/dev/null || true
rm -rf "$dst"
mv "$incoming" "$dst"
/usr/bin/open "$dst"
echo "$(date) launched"
rm -rf "$work"
rm -f "$script"
"#;
    fs::write(&script_path, helper).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("/usr/bin/nohup");
    cmd.arg("/bin/bash")
        .arg(&script_path)
        .arg(pid.to_string())
        .arg(&staged)
        .arg(dest_app)
        .arg(&work)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Safety: runs in the forked child before exec. setsid() starts a new
        // session so the dying tray cannot SIGHUP the replace helper.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn is_trusted_zip_url(url: &str) -> bool {
    let url = url.trim();
    if !url.starts_with(ZIP_PREFIX) || url.contains(['?', '#', '\\', '\n', '\r']) {
        return false;
    }
    if url.contains("..") {
        return false;
    }
    let name = url.rsplit('/').next().unwrap_or("");
    name.starts_with("NetFlash-") && name.ends_with("-macos.zip")
}

fn is_under(child: &Path, parent: &Path) -> bool {
    match (child.canonicalize(), parent.canonicalize()) {
        (Ok(c), Ok(p)) => c.starts_with(p),
        _ => false,
    }
}

fn find_app(root: &Path) -> Option<PathBuf> {
    let direct = root.join("NetFlash.app");
    if is_app_bundle(&direct) {
        return Some(direct);
    }
    let rd = fs::read_dir(root).ok()?;
    for ent in rd.flatten() {
        let p = ent.path();
        if is_app_bundle(&p) && p.file_name()?.to_str()? == "NetFlash.app" {
            return Some(p);
        }
    }
    None
}

fn is_app_bundle(path: &Path) -> bool {
    path.is_dir()
        && path.extension().and_then(|e| e.to_str()) == Some("app")
        && path.join("Contents/MacOS/netflash").is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strips_v() {
        assert_eq!(parse_version("v1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v2.0.0-beta"), Some((2, 0, 0)));
        assert!(parse_version("nope").is_none());
    }

    #[test]
    fn newer_tags() {
        assert!(is_newer("v1.1.0", "1.0.0"));
        assert!(is_newer("v1.2.0", "1.1.9"));
        assert!(!is_newer("v1.0.0", "1.0.0"));
        assert!(!is_newer("v1.0.0", "1.1.0"));
        assert!(!is_newer("trash", "1.0.0"));
    }

    #[test]
    fn zip_url_must_be_this_repo_release_asset() {
        assert!(is_trusted_zip_url(
            "https://github.com/EssekerDev/NetFlash/releases/download/v1.0.0/NetFlash-1.0.0-macos.zip"
        ));
        assert!(!is_trusted_zip_url(
            "https://evil.example/NetFlash-1.0.0-macos.zip"
        ));
        assert!(!is_trusted_zip_url(
            "https://github.com/EssekerDev/NetFlash/releases/download/v1.0.0/../evil.zip"
        ));
        assert!(!is_trusted_zip_url(
            "https://github.com/other/NetFlash/releases/download/v1.0.0/NetFlash-1.0.0-macos.zip"
        ));
    }
}
