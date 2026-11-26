//! `habbo://` protocol handler registration.

#[cfg(target_os = "linux")]
use std::process::Command;

#[cfg(windows)]
pub fn register_habbo_protocol() -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .to_string();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"SOFTWARE\Classes\habbo")
        .map_err(|e| e.to_string())?;
    key.set_value("", &"URL:Habbo Custom Launcher")
        .map_err(|e| e.to_string())?;
    key.set_value("URL Protocol", &"")
        .map_err(|e| e.to_string())?;
    let (icon, _) = key
        .create_subkey("DefaultIcon")
        .map_err(|e| e.to_string())?;
    icon.set_value("", &format!("{exe},1"))
        .map_err(|e| e.to_string())?;
    let (cmd, _) = key
        .create_subkey(r"shell\open\command")
        .map_err(|e| e.to_string())?;
    cmd.set_value("", &format!("\"{exe}\" \"%1\""))
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Body of the Linux `.desktop` entry that owns the `habbo://` scheme.
///
/// `%u` is the whole point: it is what hands the URL from the browser to the app as argv[1].
/// Without it xdg-open starts the binary with **no arguments**, the launcher sees no ticket,
/// the client starts unauthenticated and dies at ~60% of the loading bar.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Habbo Custom Launcher\nExec=\"{exe}\" %u\nTerminal=false\nMimeType=x-scheme-handler/habbo;\nCategories=Game;\nStartupWMClass=habbo-launcher-rs\n"
    )
}

#[cfg(target_os = "linux")]
pub fn register_habbo_protocol() -> Result<(), String> {
    use std::fs;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = dirs::data_dir().ok_or("no XDG data dir")?;
    let apps = data.join("applications");
    fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    let desktop = apps.join("habbo-launcher.desktop");
    let content = desktop_entry(&exe.to_string_lossy());
    fs::write(&desktop, content).map_err(|e| e.to_string())?;
    // Same file name as the packaged /usr/share/applications entry, so this user-level copy
    // overrides it instead of adding a second launcher to the menu. Old names would linger.
    for stale in ["HabboCustomLauncher.desktop", "habbo-launcher-rs.desktop"] {
        let _ = fs::remove_file(apps.join(stale));
    }
    let status = Command::new("xdg-mime")
        .args(["default", "habbo-launcher.desktop", "x-scheme-handler/habbo"])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("xdg-mime failed".into())
    }
}

#[cfg(target_os = "macos")]
pub fn register_habbo_protocol() -> Result<(), String> {
    Err("protocol registration is not supported on macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entry_hands_the_url_to_the_app() {
        let d = desktop_entry("/home/u/My Apps/habbo-launcher-linux-x64");
        assert!(
            d.lines()
                .any(|l| l == "Exec=\"/home/u/My Apps/habbo-launcher-linux-x64\" %u"),
            "Exec must be quoted and carry %u, got:\n{d}"
        );
        assert!(d.contains("MimeType=x-scheme-handler/habbo;"));
        assert!(d.contains("Terminal=false"));
    }
}
