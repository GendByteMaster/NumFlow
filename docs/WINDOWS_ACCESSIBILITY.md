# Windows Assistive Technology and protected desktops

This document is the security and deployment contract for NumFlow's Windows accessibility
integration. It is based on Microsoft's documented [Ease of Access Assistive Technology
registration](https://learn.microsoft.com/windows/win32/winauto/ease-of-access---assistive-technology-registration),
[assistive technology security](https://learn.microsoft.com/windows/win32/winauto/uiauto-securityoverview),
and [application manifest](https://learn.microsoft.com/windows/win32/sbscs/application-manifests)
requirements.

## Architecture

```text
numflow-core
├── numflow.exe
│   ├── Slint settings UI, tray, and HUD
│   ├── normal or explicit --elevated runtime
│   ├── one WH_KEYBOARD_LL owner thread
│   └── bounded ATConfig settings publisher
└── numflow-secure.exe
    ├── no UI, tray, audio, network, updater, telemetry, or shell execution
    ├── one WH_KEYBOARD_LL owner thread on its Windows-assigned desktop
    ├── NumPad normalization, motion, click, hold, and release
    └── read-only consumption of a bounded ATConfig settings snapshot
```

`numflow.exe` remains the only normal interactive application. `numflow-secure.exe` is a separate
workspace package which links only `numflow-core`, `numflow-windows`, and their Win32 support. It
rejects ordinary Default-desktop launches and accepts only the fixed `--secure-runtime` argument.

The MSI owns two machine-wide AT entries:

- `GendByteMaster_NumFlow_v1` starts `numflow.exe --background --at-runtime` and names the secure
  entry through `SecureDesktopAccommodation`;
- `GendByteMaster_NumFlowSecure_v1` starts `numflow-secure.exe --secure-runtime`.

Both entries declare the required `ApplicationName`, `Description`, `ATExe`, `StartExe`, `Profile`,
and `SimpleProfile` values. They also declare `StartParams`, `CopySettingsToLockedDesktop=1`, and
`TerminateOnDesktopSwitch=0`. The latter preserves the normal tray process started outside Ease of
Access. In accordance with Microsoft's documented non-job lifecycle, each runtime detects loss of
its input desktop, disables interception, stops motion, resets pressed-key state, releases
NumFlow-owned mouse holds, and retires its hook through the existing hook owner thread. The secure
runtime exits after cleanup; the normal runtime restores its hook idempotently when Default becomes
the input desktop again.

Because the normal process can start from the Start menu or Run key, its lifetime guard performs the
documented `AccessibilityTemp` state plus Windows+U notification handshake on start and clean exit.
That handshake is disabled when the AT registration is absent, so source and portable builds do not
invoke Ease of Access.

The MSI creates, updates, and removes the two owned AT keys declaratively. Runtime code never writes
the machine-wide AT registration.

## Settings boundary

The secure process does not read `%APPDATA%\NumFlow\config.toml`. The normal process publishes only
DWORD values under:

```text
HKCU\Software\Microsoft\Windows NT\CurrentVersion\Accessibility\ATConfig\GendByteMaster_NumFlow_v1
```

The snapshot contains an enabled-state snapshot, sanitized motion speed/acceleration/multipliers,
selected button, precision state, and one fixed numeric action code for each supported NumPad key.
The schema marker is written as invalid first and committed last. Missing, partial, malformed, or
out-of-range data fails closed. No copied value is interpreted as a path, DLL, command, or plug-in.

Windows copies this exact per-user location to the locked desktop because the AT registration sets
`CopySettingsToLockedDesktop=1`. The current Num Lock state remains the input-mode authority; the
copied enabled snapshot is an additional fail-closed gate.

## UIAccess and elevation

UIAccess and secure-desktop activation solve different problems:

- a signed `numflow.exe` built with `NUMFLOW_UIACCESS=1` embeds `uiAccess="true"` for higher-integrity
  UI on the ordinary interactive desktop;
- the default development build embeds `uiAccess="false"`, so `cargo run` remains usable outside a
  trusted installation directory;
- Windows honors UIAccess only for an Authenticode-signed executable installed in a protected
  location such as `%ProgramFiles%`;
- UIAccess does not move a process to the UAC, lock, or logon desktop and does not grant SYSTEM IL;
- `numflow.exe --elevated` remains supported until signed UIAccess behavior is validated on the
  release matrix.

The production order is: build `numflow.exe` with `NUMFLOW_UIACCESS=1`, Authenticode-sign both
executables, package them under `%ProgramFiles%\NumFlow`, sign the MSI, install it per-machine, and
then validate `uiaccess=true` and the signer chain on the installed files. No signing certificate or
private key belongs in this repository.

## Diagnostics

Input snapshots report only bounded operational fields:

```text
desktop=default|secure|locked|logon|unknown
runtime=normal|elevated|secure
integrity=low|medium|medium-plus|high|system|protected|unknown
hook_generation=N
hook_active=true|false
numpad_callbacks=N
numpad_dispatched=N
numpad_dropped=N
runtime_numpad_events=N
mouse_hold=true|false
at_registered=true|false
uiaccess=true|false
```

Desktop classification uses the current thread desktop name and documented WTS session state.
Foreground executable diagnostics are redacted outside the Default desktop. NumFlow never records
window titles, credential fields, typed keys, or secure-desktop UI contents.

## Supported and prohibited scenarios

| Scenario | Implementation status | Security boundary |
| --- | --- | --- |
| Ordinary apps on Default | Supported by existing runtime | Medium IL and UIPI apply |
| High-integrity apps on Default | Existing `--elevated`; production UIAccess path prepared | UIAccess requires signed, trusted install |
| UAC secure desktop | AT alternate runtime implemented | Must be launched by Ease of Access; UIAccess alone is insufficient |
| Lock and unlock | AT alternate runtime and WTS lifecycle implemented | Copied settings are untrusted and validated |
| Logon desktop | Registration/profile path implemented | Availability depends on Windows Ease of Access configuration and must be validated on the target Windows build |
| Credential-provider internals or SYSTEM UI automation | Not supported | NumFlow does not intercept credentials and UIAccess cannot cross SYSTEM IL |
| Remote, policy-disabled, or custom secure desktops | Not guaranteed | Windows policy is authoritative; NumFlow does not bypass it |

The secure, lock, and logon paths are not claimed as release-validated until the manual matrix in
`RELEASE_CHECKLIST.md` is completed with installed, signed artifacts. If Windows policy refuses to
launch the registered AT, NumFlow reports the limitation rather than using a service, injection,
driver, UAC disablement, or a SYSTEM launch workaround.

## Rollback

Uninstalling the MSI removes both owned AT registration keys and both executables. The per-user
`ATConfig` snapshot is non-executable settings data and may be removed separately by a future user
data cleanup option. Rolling back code before release means installing the previous MSI; do not
manually leave mixed executable and registry generations.
