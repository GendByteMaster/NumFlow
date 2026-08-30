# NumFlow Windows drivers

This directory contains the experimental Windows Input Backend v2 kernel projects. They are kept
outside the Rust workspace so WDK builds and Cargo builds remain independent.

## Safety boundary

The development workstation is build/sign/package only. Do not install, stage, load, or bind the
generated driver package there. In particular, do not run `pnputil`, `devcon`, `sc`, edit
Keyboard-class `UpperFilters`, import the test certificate into a trusted store, enable test
signing, or reboot the host for this package.

The first install/load is allowed only in the disposable VirtualBox VM described in `vm/README.md`
after the PASS_THROUGH build gate and recovery checklist pass.

## Projects

- `numflow-kbd-filter`: x64 KMDF keyboard filter skeleton. Phase 1 forwards every queued request
  unchanged and has no suppression, packet inspection, raw PDO, user-mode endpoint, or business
  logic.

Build and verification commands are documented in `docs/windows-driver-development.md`.

## TestPackage versus ProductionPackage

`TestPackage` means the disposable-VM artifact produced by `scripts/prepare-test-package.ps1`. It:

- contains the strict PASS_THROUGH driver;
- targets only the VirtualBox PS/2 compatible ID `*PNP0303`;
- uses a device-specific declarative upper filter and does not modify the Keyboard class-wide
  `UpperFilters` value;
- is signed with `CN=NumFlow Driver Test`, whose non-exportable private key remains in the current
  development user's certificate store;
- is not eligible for redistribution or host installation.

`ProductionPackage` is not implemented. It will require the intended hardware coverage,
production signing, release policy, upgrade/removal design, and physical-device acceptance gates.
Never rename or publish a `TestPackage` as a production artifact.

## Prepare the VM package

From PowerShell 7 at the repository root:

```powershell
.\driver\scripts\prepare-test-package.ps1
```

The command rebuilds Release, runs InfVerif and Inf2Cat, reuses or creates a non-exportable
CurrentUser test-signing key, signs the CAT, verifies it without changing host trust stores, and
writes exactly five files to `driver/package/x64/Release/`:

- `numflow-kbd-filter.sys`
- `numflow-kbd-filter.inf`
- `numflow-kbd-filter.cat`
- `NumFlowDriverTest.cer`
- `SHA256SUMS.txt`

`-ForceNewCertificate` creates another non-exportable certificate but never deletes an existing
one. No PFX or private-key file is created. Follow `vm/README.md` for the first install/load.
