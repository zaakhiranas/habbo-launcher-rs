//! Persistent settings, stored in the same `GlobalSettings.xml` the original launcher uses
//! so both launchers share the same configuration.

use std::fs;
use std::path::{Path, PathBuf};

pub const LAUNCHER_VERSION: &str = "36";
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36";
pub const AIR_PLUS_CLIENT_URL: &str =
    "https://github.com/LilithRainbows/HabboAirPlus/releases/download/latest/HabboAir.swf";
pub const ASSET_RAW_BASE: &str =
    "https://raw.githubusercontent.com/LilithRainbows/HabboCustomLauncher/main/Assets/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateSource {
    AirPlus,
    AirOfficial,
}

impl UpdateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateSource::AirPlus => "AIR_Plus",
            UpdateSource::AirOfficial => "AIR_Official",
        }
    }
    pub fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("AIR_Official") {
            UpdateSource::AirOfficial
        } else {
            UpdateSource::AirPlus
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            UpdateSource::AirPlus => "AIR+ (unofficial client) · Lilith release",
            UpdateSource::AirOfficial => "AIR (official hotel client)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub update_source: UpdateSource,
    pub window_scale: f32,
    pub client_air_version: String, // "latest" (AIR 51) or "old" (AIR 50)
    pub client_render_mode: String, // cpu | direct | gpu
    pub client_resolution: String,  // standard | high
}

impl Default for Settings {
    fn default() -> Self {
        let mac = cfg!(target_os = "macos");
        Settings {
            update_source: UpdateSource::AirPlus,
            window_scale: 1.0,
            client_air_version: "latest".to_string(),
            client_render_mode: if mac { "gpu" } else { "cpu" }.to_string(),
            client_resolution: "standard".to_string(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Settings::default();
        let path = settings_path();
        let Ok(text) = fs::read_to_string(&path) else {
            return s;
        };
        for (name, value) in parse_items(&text) {
            match name.as_str() {
                "UpdateSource" => s.update_source = UpdateSource::from_str(&value),
                "CustomWindowScale" => {
                    if let Ok(v) = value.parse::<f32>() {
                        if v > 0.0 {
                            s.window_scale = v;
                        }
                    }
                }
                "ClientAirVersion" => s.client_air_version = value,
                "ClientRenderMode" => s.client_render_mode = value,
                "ClientResolution" => s.client_resolution = value,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = settings_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
             <GlobalSettings>\n\
             \x20 <Item Name=\"UpdateSource\" Value=\"{}\" />\n\
             \x20 <Item Name=\"CustomWindowScale\" Value=\"{}\" />\n\
             \x20 <Item Name=\"ClientAirVersion\" Value=\"{}\" />\n\
             \x20 <Item Name=\"ClientRenderMode\" Value=\"{}\" />\n\
             \x20 <Item Name=\"ClientResolution\" Value=\"{}\" />\n\
             </GlobalSettings>\n",
            self.update_source.as_str(),
            self.window_scale,
            self.client_air_version,
            self.client_render_mode,
            self.client_resolution
        );
        fs::write(path, body)
    }
}

/// Minimal `<Item Name="X" Value="Y" />` scanner (avoids pulling in an XML dependency).
fn parse_items(xml: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chunk in xml.split("<Item").skip(1) {
        let attr = |key: &str| -> Option<String> {
            let idx = chunk.find(&format!("{key}=\""))? + key.len() + 2;
            let rest = &chunk[idx..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        };
        if let (Some(name), Some(value)) = (attr("Name"), attr("Value")) {
            out.push((name, value));
        }
    }
    out
}

/// Base data directory. `HCL_DATA_DIR` overrides it (portable install / isolated testing).
pub fn app_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HCL_DATA_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir);
        }
    }
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("."))
}

pub fn launcher_dir() -> PathBuf {
    app_data_dir().join("Habbo Launcher")
}

pub fn downloads_dir() -> PathBuf {
    let dir = launcher_dir().join("downloads");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn settings_path() -> PathBuf {
    downloads_dir().join("GlobalSettings.xml")
}

/// `downloads/air/<version>` for the official client, `downloads/airplus/<version>` for AIR+.
pub fn client_dir(source: UpdateSource, version: &str) -> PathBuf {
    let kind = match source {
        UpdateSource::AirPlus => "airplus",
        UpdateSource::AirOfficial => "air",
    };
    downloads_dir().join(kind).join(version)
}

/// Directory that holds the AIR patch zip, if the user dropped the original `Assets/` there.
pub fn assets_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("assets"));
            candidates.push(parent.join("Assets"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("assets"));
        candidates.push(cwd.join("Assets"));
    }
    candidates.into_iter().find(|p| p.is_dir())
}

pub fn cache_dir() -> PathBuf {
    let dir = launcher_dir().join("cache");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn patch_asset_name(_source: UpdateSource) -> &'static str {
    if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86") {
            "HabboAirWindowsPatch_x86.zip"
        } else {
            "HabboAirWindowsPatch_x64.zip"
        }
    } else if cfg!(target_os = "macos") {
        // caller may swap for the "old" variant when AIR 50 is selected
        "HabboAirOSXPatch_latest.zip"
    } else if cfg!(target_arch = "aarch64") {
        "HabboAirLinuxPatch_arm64.zip"
    } else {
        "HabboAirLinuxPatch_x64.zip"
    }
}

pub fn osx_patch_asset_name(air_version: &str) -> &'static str {
    if air_version == "old" {
        "HabboAirOSXPatch_old.zip"
    } else {
        "HabboAirOSXPatch_latest.zip"
    }
}

#[allow(dead_code)]
pub fn unix_exec(dir: &Path, relative: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(relative);
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o111);
            let _ = fs::set_permissions(&path, perms);
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (dir, relative);
    }
}
