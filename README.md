# Habbo Custom Launcher — Rust port

A Rust rewrite of [LilithRainbows/HabboCustomLauncher](https://github.com/LilithRainbows/HabboCustomLauncher)
(VB.NET + Avalonia, ~3k lines). Same behaviour, one native binary, no .NET runtime.

## Download

Grab the binary for your OS from the [**latest release**](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest):

| OS | File | Size |
|---|---|---|
| Windows 10/11 x64 | [`habbo-launcher-windows-x64.exe`](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest/download/habbo-launcher-windows-x64.exe) | 6.6 MB |
| **Linux x64 — .deb (recommended)** | [`habbo-launcher_1.0.1_amd64.deb`](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest/download/habbo-launcher_1.0.1_amd64.deb) | 3 MB |
| Linux x64 (portable binary) | [`habbo-launcher-linux-x64`](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest/download/habbo-launcher-linux-x64) | 7.8 MB |
| macOS Apple Silicon | [`habbo-launcher-macos-arm64`](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest/download/habbo-launcher-macos-arm64) | 5.6 MB |
| macOS Intel | [`habbo-launcher-macos-x64`](https://github.com/zaakhiranas/habbo-launcher-rs/releases/latest/download/habbo-launcher-macos-x64) | 5.8 MB |

GitHub shows each file's SHA-256 in the asset list — click the digest to copy it.

- **Windows** — double-click the exe.
- **Linux (Mint/Ubuntu/Debian)** — double-click the `.deb`. No terminal, no `chmod`: it
  installs to `/usr/bin/habbo-launcher`, adds a game-menu entry and registers the
  `habbo://` handler. Needs glibc 2.35+ (Mint 21/22, Ubuntu 22.04+); older distros get a
  dependency error instead of a mystery crash.
- **macOS / portable Linux binary** — `chmod +x <file>`, then run it. A bare binary
  downloaded through a browser has no execute bit, so double-clicking it does nothing.

Build status: [![build](https://github.com/zaakhiranas/habbo-launcher-rs/actions/workflows/build.yml/badge.svg)](https://github.com/zaakhiranas/habbo-launcher-rs/actions/workflows/build.yml)

**Credits:** the unofficial client (**AIR+**) is
[**LilithRainbows/HabboAirPlus**](https://github.com/LilithRainbows/HabboAirPlus) by
[LilithRainbows](https://github.com/LilithRainbows) — not our work. This launcher only
downloads, patches and starts it. The platform patch zips and asset layout also come from the
same author's `HabboCustomLauncher` repo.

## What it does

- **Login codes** — auto-detected from the clipboard (the code the site's **COPY** button puts
  there), pasted into the "Paste login code" box (Enter or **Use code**), or delivered by a
  `habbo://` link. When the launcher opens *from a link* it deliberately ignores whatever was
  already on the clipboard — tickets are single-use, so a leftover code only ever fails — and
  waits for the next fresh copy. A ticket handed to a client is remembered and never reused.
- **Both clients, one window** — "Launch official client (AIR)" and
  "Launch AIR+ (unofficial client) · Lilith release" each install/update and start that build
  explicitly, independent of the saved default source.
- **Client update** — resolves the newest classic (AIR) client from the selected source:
  - `AIR+` — the unofficial **HabboAirPlus** SWF by LilithRainbows, pulled from
    `https://github.com/LilithRainbows/HabboAirPlus/releases/download/latest/HabboAir.swf`
    (version = remote `x-ms-creation-time` epoch; downloaded only when newer than the local
    `<epoch>` folder)
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
- **Protocol links stop at the door** — clicking "Launch and log in" on habbo.com opens
  `habbo://hab?server=<id>&token=<ticket>`. The launcher adopts the code, shows *"Login code
  from Habbo — choose a client below"* and **waits**. It never launches on its own: the ticket is
  single-use, so the pick between official and AIR+ has to be yours. `habbo` is the same scheme
  the official Electron launcher registers, so whichever app starts last owns the link.
- **Settings** in `%APPDATA%\Habbo Launcher\downloads\GlobalSettings.xml` — same file and same
  `<Item Name Value />` schema as the original, so both launchers share config.

## Run

Single self-contained binary — no installer, no .NET, no DLLs beside it, no admin rights.

```
HabboCustomLauncher.exe        # ~6.5 MB, Windows x64
```

Copy it anywhere and double-click. First launch downloads the client (~50 MB) and the patch zip
(~8 MB) into `%APPDATA%\Habbo Launcher\`. Keep it at the path the `habbo://` registry entry
points at (`%LOCALAPPDATA%\Programs\habbo-custom-launcher\`) or website "Launch and log in" links
will open nothing.

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

This port is MIT. **It contains no upstream binaries and redistributes none of LilithRainbows'
work** — the AIR+ client (from [HabboAirPlus](https://github.com/LilithRainbows/HabboAirPlus)) and
the platform patch zips are downloaded at runtime, directly from that author's repositories, and
remain under whatever license their author applies. Credit for the unofficial client and the
patching method belongs to LilithRainbows.
