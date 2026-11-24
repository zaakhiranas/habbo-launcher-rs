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

#[cfg(target_os = "linux")]
pub fn register_habbo_protocol() -> Result<(), String> {
    use std::fs;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = dirs::data_dir().ok_or("no XDG data dir")?;
    let apps = data.join("applications");
    fs::create_dir_all(&apps).map_err(|e| e.to_string())?;
    let desktop = apps.join("HabboCustomLauncher.desktop");
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=Habbo Custom Launcher\nExec={}\nTerminal=false\nMimeType=x-scheme-handler/habbo;\nCategories=Game;\n",
        exe.to_string_lossy()
    );
    fs::write(&desktop, content).map_err(|e| e.to_string())?;
    let status = Command::new("xdg-mime")
        .args([
            "default",
            "HabboCustomLauncher.desktop",
            "x-scheme-handler/habbo",
        ])
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
