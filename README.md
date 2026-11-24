# Habbo Custom Launcher — Rust port

A Rust rewrite of [LilithRainbows/HabboCustomLauncher](https://github.com/LilithRainbows/HabboCustomLauncher)
(VB.NET + Avalonia, ~3k lines). Same behaviour, one native binary, no .NET runtime.

## What it does

- **Login codes** — auto-detected from the clipboard (the code the site's **COPY** button puts
  there), pasted into the "Paste login code" box (Enter or **Use code**), or delivered by a
  `habbo://` link. When the launcher opens *from a link* it deliberately ignores whatever was
  already on the clipboard — tickets are single-use, so a leftover code only ever fails — and
  waits for the next fresh copy. A ticket handed to a client is remembered and never reused.
- **Both clients, one window** — "Launch official client (AIR)" and
  "Launch AIR+ (unofficial client)" each install/update and start that build explicitly,
  independent of the saved default source.
- **Client update** — resolves the newest classic (AIR) client from the selected source:
  - `AIR+` (unofficial HabboAirPlus SWF, version = remote `x-ms-creation-time` epoch)
  - `AIR` (official, `https://<hotel>/gamedata/clienturls`)
- **Download + extract** — streamed download with live progress, zip extraction with the
  original skip list (`Adobe AIR`, `META-INF/signatures.xml`, `META-INF/AIR/hash`, `Habbo.exe`).
- **Patching** — applies the embedded AIR runtime patch for the host OS (fetched once from the
  upstream repo's `Assets/` and cached, or read from `./assets`), rewrites
  `META-INF/AIR/application.xml` (`versionLabel`, `versionNumber`, `renderMode`,
  `requestedDisplayResolution`) and forces the SWF byte-4 version to 51 (or 50 on macOS "old").
- **Launch** — `Habbo.exe -server <id> -ticket <ticket>` with `CREATE_NO_WINDOW`; macOS bundle
  and Linux executable paths handled.
- **Multi-instance**, **cleanup of old clients** (keeps the 3 newest), **`habbo://` protocol
  registration** (HKCU on Windows, `.desktop` + `xdg-mime` on Linux).
- **Auto-launch from the website** — clicking "Launch and log in" on habbo.com opens
  `habbo://hab?server=<id>&token=<ticket>`. When the app is started with that argument it skips
  the UI wait: update check → launch → window closes. `habbo` is the same scheme the official
  Electron launcher registers, so whichever app starts last owns the link.
- **Settings** in `%APPDATA%\Habbo Launcher\downloads\GlobalSettings.xml` — same file and same
  `<Item Name Value />` schema as the original, so both launchers share config.

## Build

```bash
cargo build --release
./target/release/habbo-launcher-rs
```

Windows: `cargo build --release` produces `habbo-launcher-rs.exe` (GUI subsystem, no console).

## Assets

The launcher needs one platform patch zip (`HabboAirWindowsPatch_x64.zip` etc., ~8 MB).
It looks in this order:

1. `./assets/<name>` (copy the upstream repo's `Assets/` folder next to the binary)
2. `%APPDATA%\Habbo Launcher\cache\<name>` (auto-downloaded and cached on first run)

## Layout

| File | Purpose |
|---|---|
| `src/main.rs` | egui app: login status, launch buttons, progress, settings window, log |
| `src/client.rs` | update source resolution, download, unzip, XML/SWF patching, launch |
| `src/login_code.rs` | login-code parsing + hotel table (unit-tested) |
| `src/config.rs` | settings XML load/save, paths, asset names |
| `src/protocol.rs` | `habbo://` registration per OS |

## Not ported

- **SWF decryption** (`AirSwfDecryptor.dll`, used to decrypt encrypted `HabboAir.swf` before
  editing on macOS). The Rust port rejects encrypted (`CWS`/`ZWS`) SWFs with a clear error
  instead of decrypting them.
- Launcher **self-updater** (`LoadingWindow` + `-updater` argument) — replace the binary manually.
- Recreating the exact Avalonia XAML skin: the UI is functionally equivalent, not pixel-identical.
- Windows 7/8 DPI quirks and the embedded Segoe UI font.

## License

Same as upstream (MIT-style). This port contains no upstream binaries; patch assets are
downloaded at runtime from the upstream repository.
