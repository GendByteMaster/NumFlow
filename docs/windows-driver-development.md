# Windows driver development

Status: host build, static-analysis, and VM TestPackage preparation workflow. This document does
not authorize driver installation or loading on the development workstation.

## Pinned toolchain

- Visual Studio Build Tools 2026 18.3
- MSVC 14.50
- Windows SDK 10.0.28000.0
- Windows Driver Kit 10.0.28000.2526
- KMDF 1.35
- x64 Debug and Release configurations

SDK and WDK build numbers must remain compatible. The project pins
`WindowsTargetPlatformVersion=10.0.28000.0` and uses the
`WindowsKernelModeDriver10.0` platform toolset.

## Build

Run the repository wrapper from PowerShell:

```powershell
.\driver\build.ps1 -Configuration Debug -Rebuild
.\driver\build.ps1 -Configuration Release -Rebuild
```

Expected outputs are under `driver/x64/<Configuration>/`. Build output is ignored by Git.
MSBuild signing remains disabled; explicit TestPackage signing is a separate script.
The wrapper uses the amd64 MSBuild executable because WDK 10.0.28000 installs the matching x64
InfVerif build-task payload. It also gives the child process a single `Path` entry so builds work in
agent hosts that expose both `Path` and `PATH`; it does not modify the parent or system environment.

## Static validation

Run INF verification against the generated INF:

```powershell
& 'C:\Program Files (x86)\Windows Kits\10\Tools\10.0.28000.0\x64\InfVerif.exe' `
  /v driver\x64\Debug\numflow-kbd-filter.inf
```

The project enables PREfast for compilation. Warnings are errors. Record the MSBuild and InfVerif
results with the phase change; never treat a generated `.sys` as permission to load it.

## Phase 1 behavior

The driver marks each created WDF device as a filter and creates one parallel default queue. Every
queued request is sent to the lower I/O target with `SEND_AND_FORGET`; buffers are not inspected or
modified. PnP and power requests that are not handled are forwarded by KMDF. D0 entry, D0 exit, and
device cleanup force the only available mode, `PASS_THROUGH`.

There is no keyboard connect callback, packet handling, suppression, raw PDO, device interface,
service IPC, heartbeat, virtual HID mouse, or Rust integration in the current driver.

## TestPackage pipeline

Run from PowerShell 7:

```powershell
.\driver\scripts\prepare-test-package.ps1
```

The pipeline dynamically finds one installed WDK version containing InfVerif, Inf2Cat, and
SignTool. It stages only the INF and SYS, generates a catalog for the Windows 11 x64 OS identifiers
supported by the installed WDK, creates or reuses an exact `CN=NumFlow Driver Test` code-signing
certificate in `Cert:\CurrentUser\My`, signs the CAT with SHA-256, and writes a SHA-256 manifest.
The private key is non-exportable. Only the public CER is exported.

The host does not trust that self-signed certificate. The pipeline therefore performs PKCS#7
signature, exact-signer, and in-memory custom-root chain checks without mutating a certificate
store. SignTool `/pa` is also run and its host trust result is reported separately; an untrusted-root
result is expected until the public CER is imported inside the disposable VM. It is forbidden to
import the CER into a host Root or TrustedPublisher store merely to turn that result into PASS.

The final TestPackage path is `driver/package/x64/Release/`. VM-only instructions are in
`driver/vm/README.md`.

## Host prohibition

On the development workstation do not:

- stage or install the INF;
- load or start the `.sys`;
- run `pnputil`, `devcon`, or `sc` for this package;
- edit Keyboard-class or device `UpperFilters`;
- enable test signing or Driver Verifier for this driver;
- reboot for driver activation.

The VM TestPackage model matches only `*PNP0303`, the VirtualBox PS/2 compatible ID, and registers a
device-specific declarative upper filter with `AddFilter`. It does not edit the class-wide
`UpperFilters` value. This narrow binding is an early VM validation mechanism, not the production
hardware strategy.
