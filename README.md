<p align="center"><img src="docs/brand/banner.png" alt="EQ Loadout"></p>

# EQ Loadout

A small Windows tray app that switches your [Equalizer APO](https://sourceforge.net/projects/equalizerapo/) EQ profile based on which application has focus. Focus a game and its profile is applied; alt-tab away and you get your default back.

- Event driven: it reacts to focus and title changes and does nothing in between (0% CPU when idle).
- Native Win32 UI, a single ~500 KB exe, no runtime to install.
- It only rewrites one file, `game.txt`. It never edits `config.txt` or your profile files.

## Anti-cheat safety

EQ Loadout never opens a handle to another process, reads its memory, enumerates its modules or injects anything. Process information comes only from:

- `CreateToolhelp32Snapshot` (exe names)
- window APIs (`GetWindowThreadProcessId`, `GetWindowTextW`, `GetClassNameW`, `GetWindowRect`, `IsWindowVisible`, `EnumWindows`, `EnumChildWindows`, `DwmGetWindowAttribute`)
- out-of-context WinEvent hooks (`EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_NAMECHANGE`)
- the `GPU Engine` performance counters, only while the rule dialog's app picker is open

## How it works

Equalizer APO reloads its configuration whenever a file in its config folder changes. You add one line to `config.txt` that includes `game.txt`. EQ Loadout then rewrites `game.txt` to include the profile of the matching rule:

```
Include: cs2.txt
```

When nothing matches, it writes your default profile, or a comment line if you have no default. It writes atomically (a temp file followed by `MoveFileExW`), so Equalizer APO never reads a half-written file, and it only writes when the content would change. When you quit, every line of `game.txt` is commented out, so no game profile stays applied.

## Setup

1. **Include game.txt.** Open `config.txt` in your Equalizer APO config folder, usually `C:\Program Files\EqualizerAPO\config`. Add this line in the section of the device you want to switch, after its `Device:` line and after your base headphone correction:

   ```
   Device: Headphones ...
   Include: My Headphones ParametricEq.txt
   Include: game.txt
   ```

   Equalizer tools such as Peace may rewrite `config.txt`. If the line disappears, EQ Loadout shows a warning in the settings window.

2. **Allow writing to the config folder.** The folder is under Program Files, so normal users can't write to it. Give your account Modify permission once, from an administrator command prompt:

   ```
   icacls "C:\Program Files\EqualizerAPO\config" /grant "%USERNAME%:(OI)(CI)M"
   ```

   You can also do this in Explorer: Properties → Security → Edit → your user → Modify. Without this permission, the settings window shows "Cannot write game.txt".

3. **Create profiles.** A profile is any `.txt` file in the config folder that contains Equalizer APO filter lines, for example `cs2.txt`:

   ```
   Filter: ON PK Fc 4000 Hz Gain 3 dB Q 1.4
   ```

4. **Run EQ Loadout.** Download `eqloadout.exe` from [Releases](https://github.com/luanfernandes/eqloadout/releases), copy it somewhere permanent and run it. Right-click the tray icon to open **Settings** and add rules. Turn on **Start with Windows** from the same menu. It adds a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that points at the exe you enabled it from.

## Rules

A rule has an exe name (exact, case-insensitive, e.g. `cs2.exe`), a window-title substring (case-insensitive), or both, plus a profile. If a rule sets both, both must match. Rules are checked top to bottom and the first match wins, so use **Move up** and **Move down** to order them.

The rule dialog helps you fill in the fields:

- **Pick running app** lists open windows, with likely games first (GPU 3D usage, windows covering a whole monitor, and a penalty for browsers, Discord, Steam, OBS and similar).
- **Capture focused app** waits 5 seconds, then reads whichever window has focus.
- **Select exe...** opens a file picker. It starts in your Steam library's `steamapps\common` folder if Steam is installed.

Store apps and Game Pass games run inside `ApplicationFrameHost.exe`. EQ Loadout resolves the app behind that host, so rules use the real exe name.

## Configuration file

Settings live in `%APPDATA%\eqloadout\config.toml`. The file is created on first run.

```toml
eapo_config_dir = 'C:\Program Files\EqualizerAPO\config'
default_profile = "flat.txt"

[[rules]]
exe = "cs2.exe"
profile = "cs2.txt"

[[rules]]
title = "Minecraft"
profile = "minecraft.txt"
```

`eapo_config_dir` defaults to the `ConfigPath` value under `HKLM\SOFTWARE\EqualizerAPO`. If the file is broken, EQ Loadout keeps running with defaults, shows the error in the settings window and leaves the file untouched. Edits made by hand are picked up on the next start. Edits made in the settings window apply as soon as you save.

## Building

Requires Rust (MSVC toolchain) and the Windows SDK (for `rc.exe`).

```
cargo build --release
cargo test
```

Debug builds keep a console and log every focus change and write. Ctrl+C quits cleanly. To print the ranked app list used by the picker:

```
eqloadout --list-apps
```

## Manual test checklist

- [ ] Alt-tab into a game with a rule: the tray tooltip and `game.txt` switch to its profile. Alt-tab out: the default comes back. Alt-tabbing within the game (the task switcher overlay) does not switch anything.
- [ ] Title change while focused: a title-only rule starts matching when the game changes its window title after launch.
- [ ] Close the game: the next focused app gets the default profile.
- [ ] A Game Pass or Store game: the rule matches its real exe, not `ApplicationFrameHost.exe`.
- [ ] Pause from the tray: the tray icon turns grey, the default is written once and focus changes are ignored. Resume: the icon turns green and the current window is matched again.
- [ ] Unwritable config folder (remove the Modify permission): the tray icon turns amber, the settings status line and the tooltip say the folder needs Modify permission, and the app keeps running.
- [ ] Missing profile file (rename a rule's profile): the default is applied, the tray icon turns amber and a warning appears in the tooltip and the settings window.
- [ ] Start a second copy: the settings window of the running copy opens.
- [ ] Quit: `game.txt` is commented out.

## Planned

- Reload `config.toml` automatically when it is edited by hand.

## License

[MIT](LICENSE)
