# Windows keyboard filter Phase 1 validation

Date: 2026-08-29

Branch: `dev/windows-input-backend-v2`

Scope: host build and static validation only; no install or load.

## Toolchain evidence

- Visual Studio Build Tools 2026 18.3
- amd64 MSBuild 18.3.0.7010
- MSVC 14.50.35717
- Windows SDK 10.0.28000.0
- Windows Driver Kit / InfVerif 10.0.28000.2526
- KMDF 1.35

## Driver build gate

Commands:

```powershell
.\driver\build.ps1 -Configuration Debug -Rebuild
.\driver\build.ps1 -Configuration Release -Rebuild
```

Result for both configurations: PASS.

- x64 `.sys` produced;
- INF stamped;
- PREfast enabled (`/analyze` with `WindowsPrefast.dll` and `drivers.dll`);
- compiler warning level 4 with warnings as errors;
- signability test: no errors, no warnings;
- unsigned catalog generated;
- no certificate generated or installed.

## INF gate

`InfVerif.exe /v` was run against the generated Debug and Release INF files.

Result for both configurations: `INF is VALID`, exit code 0.

At the time of this original Phase 1 gate, the model used the deliberately non-existent
`NUMFLOW\KBD_FILTER_PHASE1_DO_NOT_INSTALL` hardware ID. The later VM TestPackage replaces it with a
VirtualBox-only `*PNP0303` model; see `windows-driver-development.md`. The original unsigned gate
evidence below remains historical evidence and is not a description of the current staged package.

## Existing backend regression gate

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
```

Results:

- formatting: PASS;
- clippy: PASS;
- tests: 103 passed, 0 failed, 1 ignored;
- ignored test: interactive Windows desktop singleton/liveness test.

No Rust application, core, UI, `WH_KEYBOARD_LL`, or `SendInput` source changed in Phase 1.

## Host safety evidence

After the builds:

- Debug and Release `.sys` files: `NotSigned`;
- Debug and Release `.cat` files: `NotSigned`;
- Windows service `numflow-kbd-filter`: absent (`OpenService` error 1060);
- NumFlow driver package in Driver Store: absent;
- Keyboard class `UpperFilters`: `kbdclass` only;
- no `pnputil`, `devcon`, service creation, registry mutation, test-signing change, driver load, or
  driver-related reboot was performed.

## Gate decision

Phase 1 build/static-analysis gate: PASS.

Host installation remains prohibited. The first install/load remains blocked until the recovery
checklist is completed in the guarded disposable VirtualBox VM. Packet observation, connect callback,
service communication, suppression, ACTIVE mode, and virtual HID output remain out of scope.
