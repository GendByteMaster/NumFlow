# Installing NumFlow on Windows

NumFlow v0.1 is currently released for **Windows x64 only**. Linux and macOS support is planned for future versions, but there are no supported Linux or macOS installation packages yet.

NumFlow is distributed in two Windows x64 formats:

- `NumFlow-<version>-x64.msi` — recommended installed build.
- `NumFlow-<version>-portable-x64.zip` — portable build that does not modify the installed-app list.

## MSI installation

The MSI installs NumFlow per-machine to:

```text
C:\Program Files\NumFlow\NumFlow.exe
```

Because the package installs under Program Files, Windows may request administrator approval during installation. The installer creates a Start Menu shortcut and registers NumFlow in Windows Installed Apps so it can be removed through normal Windows settings.

User configuration is stored separately from the executable at:

```text
%APPDATA%\NumFlow\config.toml
```

Updating or reinstalling the application therefore does not require moving the user's profiles, bindings, HUD preference, sound preference, or pointer settings.

## Portable build

Extract the portable ZIP to a stable folder and run `NumFlow.exe` directly. The ZIP includes the executable, project license, README, and the UI SFX license/source notice.

If `Start with Windows` is enabled while using the portable build, do not move or delete the executable afterward without first disabling that setting. The Windows startup registration points to the executable path that was current when the preference was applied.

## Start with Windows

NumFlow's autostart setting is controlled by NumFlow itself. The MSI does not force NumFlow to start with Windows.

When enabled, NumFlow writes the current executable to the current-user Windows Run key:

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Run\NumFlow
```

The registered command uses:

```text
"C:\...\NumFlow.exe" --background
```

`--background` starts the global keyboard runtime, tray icon, HUD support, and saved configuration without opening the settings window. This is independent of the normal `Start minimized` preference. The startup registration is per-user and does not require administrator rights.

Turning `Start with Windows` off removes the Run entry. Resetting all NumFlow settings also attempts to remove the startup registration.

## Manual background launch

The same startup mode can be used manually:

```powershell
.\NumFlow.exe --background
```

If NumFlow is already running, single-instance protection causes the second process to exit.

## Uninstall

Remove the installed build from Windows **Settings → Apps → Installed apps → NumFlow** or with standard MSI tooling.

User configuration is intentionally separate from the installation directory. The MSI does not own or delete `%APPDATA%\NumFlow\config.toml`.

The current MSI also does not own the per-user Run entry created by NumFlow itself. If `Start with Windows` is enabled, disable it in NumFlow before uninstalling so the Run entry is removed cleanly.

## Integrity verification

Tagged GitHub releases include `SHA256SUMS.txt`. Compare the SHA-256 hash of the downloaded MSI or portable ZIP with the matching entry when integrity verification is required.

## Current signing status

The current release pipeline produces unsigned artifacts. Windows may therefore show SmartScreen or publisher warnings. Production code signing is a separate release-readiness item and should be added only with a protected code-signing certificate workflow; private signing material must never be committed to the repository.

## Future platforms

Linux and macOS support is part of the future platform direction. Their input backends, startup integration, packaging, and release validation will be implemented separately rather than treating the current Windows installer or Win32 input backend as cross-platform.
