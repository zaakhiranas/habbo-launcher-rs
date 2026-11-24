//! Client resolution, download, extraction and patching — the Rust equivalent of
//! `CheckUpdatesAndLaunchClient` / `UpdateClient` / `LaunchClient` from the VB original.

use crate::config::*;
use crate::login_code::LoginCode;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Status callback: `(percent 0..=100, message)`.
pub type Progress<'a> = &'a dyn Fn(u8, &str);

#[derive(Debug, Clone)]
pub struct RemoteClient {
    pub version: String,
    pub url: String,
}

fn http(timeout: Option<Duration>) -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .gzip(true);
    if let Some(t) = timeout {
        builder = builder.timeout(t);
    }
    builder.build().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- remote info

/// `x-ms-creation-time` header (Blob storage) as epoch seconds — the AIR+ client version id.
fn remote_last_modified_epoch(url: &str) -> Result<String, String> {
    let client = http(Some(Duration::from_secs(15)))?;
    let resp = client.head(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} for {url}", resp.status()));
    }
    let raw = resp
        .headers()
        .get("x-ms-creation-time")
        .and_then(|v| v.to_str().ok())
        .ok_or("Last modified header not found")?
        .to_string();
    parse_rfc1123(&raw)
        .map(|secs| secs.to_string())
        .ok_or_else(|| format!("Unparsable creation time: {raw}"))
}

fn parse_rfc1123(s: &str) -> Option<i64> {
    // "Wed, 21 Oct 2015 07:28:00 GMT"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let day: i64 = parts[1].parse().ok()?;
    let month = match parts[2].to_ascii_lowercase().as_str() {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    };
    let year: i64 = parts[3].parse().ok()?;
    let clock: Vec<&str> = parts[4].split(':').collect();
    if clock.len() != 3 {
        return None;
    }
    let (h, m, sec): (i64, i64, i64) = (
        clock[0].parse().ok()?,
        clock[1].parse().ok()?,
        clock[2].parse().ok()?,
    );
    let days = days_from_civil(year, month, day);
    Some(days * 86400 + h * 3600 + m * 60 + sec)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn latest_local_version(source: UpdateSource) -> Option<String> {
    let kind = match source {
        UpdateSource::AirPlus => "airplus",
        UpdateSource::AirOfficial => "air",
    };
    let root = downloads_dir().join(kind);
    let mut best: Option<(SystemTime, String)> = None;
    for entry in fs::read_dir(root).ok()? {
        let Ok(entry) = entry else { continue };
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let swf = dir.join("HabboAir.swf");
        let Ok(meta) = fs::metadata(&swf) else {
            continue;
        };
        let modified = meta.modified().unwrap_or(UNIX_EPOCH);
        if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            if let Some(name) = dir.file_name().and_then(|n| n.to_str()) {
                best = Some((modified, name.to_string()));
            }
        }
    }
    best.map(|(_, v)| v)
}

pub fn fetch_remote_client(
    source: UpdateSource,
    server_url: &str,
    progress: Progress,
) -> Result<RemoteClient, String> {
    match source {
        UpdateSource::AirPlus => {
            progress(0, "Checking AIR+ client version");
            let version = remote_last_modified_epoch(AIR_PLUS_CLIENT_URL)
                .or_else(|_| latest_local_version(source).ok_or("no local client".to_string()))?;
            Ok(RemoteClient {
                version,
                url: AIR_PLUS_CLIENT_URL.to_string(),
            })
        }
        UpdateSource::AirOfficial => {
            progress(0, "Fetching client urls from hotel");
            let json = match get_json(&format!("https://{server_url}/gamedata/clienturls")) {
                Ok(j) => j,
                Err(_) => {
                    let version =
                        latest_local_version(source).ok_or("no local client".to_string())?;
                    format!(
                        "{{\"flash-windows-version\":\"{version}\",\"flash-windows\":\"{AIR_PLUS_CLIENT_URL}\"}}"
                    )
                }
            };
            let value: serde_json::Value =
                serde_json::from_str(&json).map_err(|e| format!("Bad clienturls json: {e}"))?;
            let version = value
                .get("flash-windows-version")
                .and_then(|v| v.as_str())
                .ok_or("flash-windows-version missing")?
                .to_string();
            let url = value
                .get("flash-windows")
                .and_then(|v| v.as_str())
                .ok_or("flash-windows missing")?
                .to_string();
            Ok(RemoteClient { version, url })
        }
    }
}

fn get_json(url: &str) -> Result<String, String> {
    let client = http(Some(Duration::from_secs(15)))?;
    let resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    resp.text().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- assets

/// Returns the patch zip bytes: from `./assets`, then the app dir, else downloaded and cached.
pub fn patch_asset(name: &str) -> Result<Vec<u8>, String> {
    if let Some(dir) = assets_dir() {
        let path = dir.join(name);
        if path.is_file() {
            return fs::read(&path).map_err(|e| e.to_string());
        }
    }
    let cached = cache_dir().join(name);
    if cached.is_file() {
        if let Ok(bytes) = fs::read(&cached) {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }
    let url = format!("{ASSET_RAW_BASE}{name}");
    let client = http(None)?;
    let mut resp = client.get(&url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Could not download {name}: HTTP {}", resp.status()));
    }
    let mut bytes = Vec::new();
    resp.copy_to(&mut bytes).map_err(|e| e.to_string())?;
    let _ = fs::write(&cached, &bytes);
    Ok(bytes)
}

/// XXH3-64 of the patch asset, matching what the VB launcher writes to `AIR_HASH.txt`.
pub fn asset_hash(name: &str) -> Result<String, String> {
    let bytes = patch_asset(name)?;
    Ok(format!("{:016X}", xxhash_rust::xxh3::xxh3_64(&bytes)))
}

fn effective_patch_name(source: UpdateSource, air_version: &str) -> &'static str {
    if cfg!(target_os = "macos") {
        osx_patch_asset_name(air_version)
    } else {
        patch_asset_name(source)
    }
}

// ---------------------------------------------------------------- install state

pub fn is_client_version_exists(source: UpdateSource, version: &str) -> bool {
    let dir = client_dir(source, version);
    let Ok(expected) = asset_hash(effective_patch_name(source, "latest")) else {
        return false;
    };
    let Ok(stored) = fs::read_to_string(dir.join("AIR_HASH.txt")) else {
        return false;
    };
    dir.is_dir() && stored.trim() == expected
}

// ---------------------------------------------------------------- download

fn download_file(
    url: &str,
    dest: &Path,
    progress: Progress,
    from: u8,
    to: u8,
) -> Result<(), String> {
    let client = http(None)?;
    let resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut reader = resp;
    let mut file = fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut last_pct = 255u8;
    loop {
        let read = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        file.write_all(&buf[..read]).map_err(|e| e.to_string())?;
        done += read as u64;
        if let Some(scaled) = (done * (to - from) as u64).checked_div(total) {
            let pct = (from as u64 + scaled) as u8;
            if pct != last_pct {
                last_pct = pct;
                progress(pct, &format!("Downloading client ({pct}%)"));
            }
        } else {
            progress(
                from,
                &format!("Downloading client ({} MB)", done / 1_048_576),
            );
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}

// ---------------------------------------------------------------- unzip

fn unzip_bytes(
    bytes: &[u8],
    dest: &Path,
    skip: &[&str],
    progress: Progress,
    msg: &str,
) -> Result<(), String> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let entry_count = archive.len();
    for i in 0..entry_count {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if skip
            .iter()
            .any(|s| rel_str == *s || rel_str.starts_with(&format!("{s}/")))
        {
            continue;
        }
        let out_path = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = fs::File::create(&out_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out_path, fs::Permissions::from_mode(mode | 0o600));
        }
        if entry_count > 50 && i % 25 == 0 {
            progress(80, msg);
        }
    }
    progress(80, msg);
    Ok(())
}

// ---------------------------------------------------------------- XML patching

fn element_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}");
    let mut idx = 0;
    while let Some(found) = xml[idx..].find(&open) {
        let start = idx + found;
        let after = start + open.len();
        let boundary = xml[after..].chars().next()?;
        if !(boundary == '>' || boundary == ' ' || boundary == '/') {
            idx = after;
            continue;
        }
        let gt = xml[after..].find('>')? + after;
        if xml[..gt].ends_with('/') {
            return Some("");
        }
        let close = format!("</{tag}>");
        let end = xml[gt..].find(&close)? + gt;
        return Some(&xml[gt + 1..end]);
    }
    None
}

/// Sets `<tag>value</tag>`, converting `<tag />` into an explicit pair.
fn set_element_text(xml: &str, tag: &str, value: &str) -> (String, bool) {
    let open = format!("<{tag}");
    let mut idx = 0;
    while let Some(found) = xml[idx..].find(&open) {
        let start = idx + found;
        let after = start + open.len();
        let Some(boundary) = xml[after..].chars().next() else {
            break;
        };
        if !(boundary == '>' || boundary == ' ' || boundary == '/') {
            idx = after;
            continue;
        }
        let Some(gt_rel) = xml[after..].find('>') else {
            break;
        };
        let gt = after + gt_rel;
        if xml[..gt].ends_with('/') {
            // self-closing `<tag />`: these elements only ever hold text, so drop the tag shell
            let mut out = String::with_capacity(xml.len() + value.len());
            out.push_str(&xml[..start]);
            out.push_str(&format!("<{tag}>"));
            out.push_str(value);
            out.push_str(&format!("</{tag}>"));
            out.push_str(&xml[gt + 1..]);
            return (out, true);
        }
        let close = format!("</{tag}>");
        let Some(end_rel) = xml[gt..].find(&close) else {
            break;
        };
        let end = gt + end_rel;
        let mut out = String::with_capacity(xml.len());
        out.push_str(&xml[..gt + 1]);
        out.push_str(value);
        out.push_str(&xml[end..]);
        return (out, true);
    }
    (xml.to_string(), false)
}

fn insert_into_initial_window(xml: &str, tag: &str, value: &str) -> Option<String> {
    let close = "</initialWindow>";
    let pos = xml.find(close)?;
    let mut out = String::with_capacity(xml.len() + 64);
    out.push_str(&xml[..pos]);
    out.push_str(&format!("<{tag}>{value}</{tag}>"));
    out.push_str(&xml[pos..]);
    Some(out)
}

fn update_air_application_xml(
    client_dir: &Path,
    is_post_install: bool,
    settings: &Settings,
    version: &str,
    log: Progress,
) -> Result<(), String> {
    let mut base = client_dir.to_path_buf();
    if is_post_install && cfg!(target_os = "macos") {
        base = base.join("Habbo.app").join("Contents").join("Resources");
    }
    let orig_path = base.join("META-INF").join("AIR").join("application.xml");
    let new_path = base.join("application.xml");

    // The AIR+ patch ships placeholders (`XML_CLIENT_VERSION`); AIR rejects those as a
    // descriptor, so a placeholder means "use the real client version" instead.
    let read_tag = |tag: &str| {
        fs::read_to_string(&orig_path)
            .ok()
            .and_then(|x| element_text(&x, tag).map(|s| s.trim().to_string()))
            .filter(|v| !v.is_empty() && !v.contains("XML_CLIENT_"))
    };

    // `versionNumber` must be a dotted version ("1.0.0") — AIR refuses a bare integer
    // such as the AIR+ epoch build id with "invalid application descriptor". Label and
    // number are derived independently so re-running the patch is idempotent.
    let version_label = read_tag("versionLabel").unwrap_or_else(|| version.to_string());
    let version_number = read_tag("versionNumber")
        .filter(|v| v.contains('.'))
        .unwrap_or_else(|| dotted_version(version));

    let mut xml = match fs::read_to_string(&new_path) {
        Ok(x) => x,
        Err(_) => match fs::read_to_string(&orig_path) {
            Ok(x) => x,
            Err(e) => return Err(format!("application.xml not found: {e}")),
        },
    };

    for (tag, value) in [
        ("versionLabel", version_label.as_str()),
        ("versionNumber", version_number.as_str()),
    ] {
        let (next, ok) = set_element_text(&xml, tag, value);
        if !ok {
            log(
                80,
                &format!("warning: <{tag}> not found in application.xml"),
            );
        }
        xml = next;
    }

    for (tag, value) in [
        (
            "requestedDisplayResolution",
            settings.client_resolution.as_str(),
        ),
        ("renderMode", settings.client_render_mode.as_str()),
    ] {
        let (next, ok) = set_element_text(&xml, tag, value);
        xml = if ok {
            next
        } else {
            insert_into_initial_window(&xml, tag, value).unwrap_or_else(|| {
                log(
                    80,
                    &format!("warning: <initialWindow> missing, cannot set {tag}"),
                );
                xml.clone()
            })
        };
    }

    if let Some(parent) = orig_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&orig_path, xml).map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&new_path);
    Ok(())
}

/// AIR requires `versionNumber` to be dotted (`1.2.3`); a bare build id is rejected as an
/// invalid descriptor, so non-dotted build ids fall back to a valid placeholder version.
fn dotted_version(version: &str) -> String {
    if version.contains('.') {
        version.to_string()
    } else {
        "1.0.0".to_string()
    }
}

fn replace_swf_version(swf_path: &Path, new_version: u8) -> Result<(), String> {
    let mut bytes = fs::read(swf_path).map_err(|e| format!("{}: {e}", swf_path.display()))?;
    if bytes.len() < 8 {
        return Err("HabboAir.swf too small".into());
    }
    let signature = String::from_utf8_lossy(&bytes[..3]).to_string();
    if !matches!(signature.as_str(), "FWS" | "CWS" | "ZWS") {
        return Err(format!("Not a SWF file (signature {signature:?})"));
    }
    if bytes[3] != new_version {
        bytes[3] = new_version;
        fs::write(swf_path, bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------------------------------------------------------------- update

/// Kills only processes whose executable lives at `exe` — never another installed client.
///
/// Paths are compared after `canonicalize`, because Windows reports process image paths
/// using 8.3 short names (`ZAAKHI~1`) while our own paths come from the long name.
fn kill_client_by_path(exe: &Path) {
    let Ok(target) = exe.canonicalize() else {
        // client not installed / already removed: nothing to kill
        return;
    };
    let mut sys = sysinfo::System::new_all();
    sys.refresh_all();
    for process in sys.processes().values() {
        let Some(path) = process.exe() else { continue };
        if path == target || path.canonicalize().map(|p| p == target).unwrap_or(false) {
            process.kill();
        }
    }
}

pub fn update_client(
    settings: &Settings,
    remote: &RemoteClient,
    progress: Progress,
) -> Result<(), String> {
    let dir = client_dir(settings.update_source, &remote.version);
    progress(1, "Preparing client folder");
    if dir.exists() {
        kill_client_by_path(&dir.join("Habbo.exe"));
        fs::remove_dir_all(&dir).map_err(|e| format!("cannot clear {}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let is_plus = settings.update_source == UpdateSource::AirPlus;
    let download_path = dir.join(if is_plus {
        "HabboAir.swf"
    } else {
        "ClientDownload.zip"
    });
    download_file(&remote.url, &download_path, progress, 2, 75)?;

    progress(76, "Extracting client");
    let patch_name = effective_patch_name(settings.update_source, &settings.client_air_version);
    let patch = patch_asset(patch_name)?;
    unzip_bytes(&patch, &dir, &[], progress, "Extracting AIR patch")?;

    if is_plus {
        let plus = patch_asset("HabboAirPlusPatch.zip")?;
        unzip_bytes(&plus, &dir, &[], progress, "Applying AIR+ patch")?;
    } else {
        let skip = [
            "Adobe AIR",
            "META-INF/signatures.xml",
            "META-INF/AIR/hash",
            "Habbo.exe",
        ];
        let bytes = fs::read(&download_path).map_err(|e| e.to_string())?;
        unzip_bytes(&bytes, &dir, &skip, progress, "Extracting client archive")?;
        let _ = fs::remove_file(&download_path);
    }

    fs::create_dir_all(dir.join("META-INF").join("AIR")).map_err(|e| e.to_string())?;
    progress(85, "Patching application.xml");
    update_air_application_xml(&dir, false, settings, &remote.version, progress)?;

    let license = dir.join("license.txt");
    if license.is_file() {
        let _ = fs::rename(
            &license,
            dir.join("META-INF").join("AIR").join("license.txt"),
        );
    }

    progress(88, "Patching SWF version");
    let swf_version = if cfg!(target_os = "macos") && settings.client_air_version == "old" {
        50
    } else {
        51
    };
    replace_swf_version(&dir.join("HabboAir.swf"), swf_version)?;

    #[cfg(target_os = "macos")]
    {
        fix_osx_structure(&dir);
        unix_exec(&dir, "Habbo.app/Contents/MacOS/Habbo");
        let _ = Command::new("codesign")
            .args([
                "--force",
                "--deep",
                "--sign",
                "-",
                dir.join("Habbo.app").to_string_lossy().as_ref(),
            ])
            .status();
    }
    #[cfg(target_os = "linux")]
    unix_exec(&dir, "Habbo");

    fs::write(dir.join("AIR_HASH.txt"), asset_hash(patch_name)?).map_err(|e| e.to_string())?;
    fs::write(dir.join("VERSION.txt"), &remote.version).map_err(|e| e.to_string())?;
    progress(95, "Client ready");
    Ok(())
}

#[cfg(target_os = "macos")]
fn fix_osx_structure(dir: &Path) {
    // Some client archives ship the .app bundle flat; move it under Habbo.app/Contents.
    let app = dir.join("Habbo.app");
    if !app.is_dir() {
        let _ = fs::create_dir_all(app.join("Contents").join("Resources"));
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "Habbo.app" {
                continue;
            }
            let _ = fs::rename(&path, app.join("Contents").join("Resources").join(&name));
        }
    }
}

pub fn clean_deprecated_clients(source: UpdateSource, keep: usize) -> Result<(), String> {
    let kind = match source {
        UpdateSource::AirPlus => "airplus",
        UpdateSource::AirOfficial => "air",
    };
    let root = downloads_dir().join(kind);
    let Ok(entries) = fs::read_dir(&root) else {
        return Ok(());
    };
    let mut dirs: Vec<(SystemTime, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(meta) = fs::metadata(dir.join("HabboAir.swf")) else {
            continue;
        };
        dirs.push((meta.modified().unwrap_or(UNIX_EPOCH), dir));
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.0));
    for (_, dir) in dirs.into_iter().skip(keep) {
        kill_client_by_path(&dir.join("Habbo.exe"));
        let _ = fs::remove_dir_all(dir);
    }
    Ok(())
}

// ---------------------------------------------------------------- launch

pub fn client_executable(dir: &Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        dir.join("Habbo.exe")
    } else if cfg!(target_os = "macos") {
        dir.join("Habbo.app")
            .join("Contents")
            .join("MacOS")
            .join("Habbo")
    } else {
        dir.join("Habbo")
    }
}

/// Arguments handed to the AIR client: `-server <hotel> -ticket <sso ticket>`.
pub fn launch_args(login: &LoginCode) -> Vec<String> {
    vec![
        "-server".to_string(),
        login.server_id.clone(),
        "-ticket".to_string(),
        login.sso_ticket.clone(),
    ]
}

pub fn launch_client(
    settings: &Settings,
    version: &str,
    login: &LoginCode,
    progress: Progress,
) -> Result<(), String> {
    progress(96, "Cleaning old clients");
    let _ = clean_deprecated_clients(settings.update_source, 3);

    let dir = client_dir(settings.update_source, version);
    if cfg!(target_os = "macos") {
        let resources = dir.join("Habbo.app").join("Contents").join("Resources");
        let orig = resources
            .join("META-INF")
            .join("AIR")
            .join("application.xml");
        let new = resources.join("application.xml");
        if orig.is_file() {
            let _ = fs::copy(&orig, &new);
        }
        update_air_application_xml(&dir, true, settings, version, progress)?;
    } else {
        let orig = dir.join("META-INF").join("AIR").join("application.xml");
        let new = dir.join("application.xml");
        if orig.is_file() {
            let _ = fs::copy(&orig, &new);
        }
        update_air_application_xml(&dir, false, settings, version, progress)?;
    }

    let swf_version = if cfg!(target_os = "macos") && settings.client_air_version == "old" {
        50
    } else {
        51
    };
    replace_swf_version(&dir.join("HabboAir.swf"), swf_version)?;

    progress(98, "Launching client");
    let exe = client_executable(&dir);
    if !exe.is_file() {
        return Err(format!("client executable missing: {}", exe.display()));
    }
    let mut cmd = Command::new(&exe);
    cmd.current_dir(&dir).args(launch_args(login));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
        .map_err(|e| format!("could not start {}: {e}", exe.display()))?;
    progress(100, "Client launched");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_args_match_client_contract() {
        let login = LoginCode::parse("hhes.aaaa-bbbb-cccc.V4.TestUser").unwrap();
        assert_eq!(
            launch_args(&login),
            vec!["-server", "hhes", "-ticket", "aaaa-bbbb-cccc.V4"]
        );
    }

    #[test]
    fn rfc1123_dates_convert() {
        assert_eq!(
            parse_rfc1123("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(1445412480)
        );
    }

    #[test]
    fn version_number_must_be_dotted() {
        assert_eq!(dotted_version("1793809284"), "1.0.0");
        assert_eq!(dotted_version("1.0.31"), "1.0.31");
    }

    #[test]
    fn xml_elements_are_rewritten() {
        let xml = "<a><versionLabel>old</versionLabel><initialWindow><renderMode>gpu</renderMode><requestedDisplayResolution /></initialWindow></a>";
        let (xml, ok) = set_element_text(xml, "versionLabel", "new");
        assert!(ok);
        assert!(xml.contains("<versionLabel>new</versionLabel>"));
        let (xml, ok) = set_element_text(&xml, "requestedDisplayResolution", "high");
        assert!(ok);
        assert!(xml.contains("<requestedDisplayResolution>high</requestedDisplayResolution>"));
        assert!(!xml.contains("<requestedDisplayResolution />"));
        assert_eq!(element_text(&xml, "renderMode"), Some("gpu"));
    }

    #[test]
    fn missing_element_falls_back_to_initial_window_insert() {
        let xml = "<a><initialWindow><title>Habbo</title></initialWindow></a>";
        let (out, ok) = set_element_text(xml, "renderMode", "cpu");
        assert!(!ok);
        let out = insert_into_initial_window(&out, "renderMode", "cpu").unwrap();
        assert!(out.contains("<renderMode>cpu</renderMode></initialWindow>"));
    }
}

#[cfg(test)]
mod kill_tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn kills_only_the_given_executable_path() {
        let dir = std::env::temp_dir().join("hcl-rs-killtest");
        let _ = fs::create_dir_all(&dir);
        let exe = dir.join("fake_habbo.exe");
        fs::copy(r"C:\Windows\System32\ping.exe", &exe).unwrap();
        let other = dir.join("other_habbo.exe");
        fs::copy(r"C:\Windows\System32\ping.exe", &other).unwrap();

        let mut victim = Command::new(&exe)
            .args(["-n", "60", "127.0.0.1"])
            .spawn()
            .unwrap();
        let mut survivor = Command::new(&other)
            .args(["-n", "60", "127.0.0.1"])
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(600));

        kill_client_by_path(&exe);
        std::thread::sleep(std::time::Duration::from_millis(900));

        let victim_done = victim.try_wait().unwrap().is_some();
        let survivor_done = survivor.try_wait().unwrap().is_some();
        let _ = survivor.kill();
        let _ = fs::remove_dir_all(&dir);
        assert!(victim_done, "process at the target path should be killed");
        assert!(!survivor_done, "process at another path must not be killed");
    }
}
