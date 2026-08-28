# NumFlow Windows release process

NumFlow's Windows distribution pipeline is defined in `.github/workflows/release.yml` and is intentionally separate from the normal development CI gate.

## Outputs

A validated build produces:

- `NumFlow-<version>-x64.msi` — WiX Toolset 4 per-machine installer.
- `NumFlow-<version>-portable-x64.zip` — portable archive.
- `SHA256SUMS.txt` — SHA-256 checksums for both distribution artifacts.

The portable archive contains `NumFlow.exe`, `LICENSE`, `README.md`, and `SFX-LICENSE.md`. Runtime assets such as the application icon and UI sounds are embedded in the executable by the Rust build.

## Pull request validation

Pull requests targeting `master` run both the standard Windows CI gate and the Windows distribution workflow. The distribution workflow performs formatting, Clippy, tests, a locked release build, WiX MSI creation, MSI decompilation verification, portable archive verification, and checksum generation.

The release PR is therefore the packaging validation point without publishing a public release.

## Creating a tagged release

1. Ensure `Cargo.toml` contains the intended version.
2. Complete the required manual items in `docs/RELEASE_CHECKLIST.md`.
3. Merge the approved release PR from `dev/master` to `master` only after explicit approval.
4. Create and push a tag that exactly matches the Cargo package version, for example:

   ```powershell
   git tag v0.1.0
   git push origin v0.1.0
   ```

5. The release workflow rejects a tag that does not equal `v<Cargo.toml version>`.
6. For a valid tag it builds the MSI, portable ZIP, and checksums, uploads the workflow artifact, and creates or updates the matching GitHub Release.

## Local MSI build

Requirements:

- Rust 1.98 as pinned by the repository.
- .NET SDK.
- WiX Toolset CLI 4.0.6.

Example:

```powershell
cargo build --locked --workspace --release --all-features
dotnet tool install --global wix --version 4.0.6

$version = (cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json).packages |
  Where-Object { $_.name -eq "numflow" } |
  Select-Object -ExpandProperty version -First 1

wix build installer/NumFlow.wxs -arch x64 `
  -d "ProductVersion=$version" `
  -d "NumFlowExe=$((Resolve-Path target/release/numflow.exe).Path)" `
  -d "NumFlowSecureExe=$((Resolve-Path target/release/numflow-secure.exe).Path)" `
  -pdbtype none `
  -out "NumFlow-$version-x64.msi"
```

The CI pipeline additionally decompiles the resulting MSI as a structural packaging check and verifies the required portable ZIP contents.

## Versioning and upgrades

`installer/NumFlow.wxs` has a stable `UpgradeCode`, while WiX creates the package/product identity required for each build. `MajorUpgrade` blocks downgrades and allows a newer NumFlow MSI to replace an older installed version.

The version passed to WiX comes from the root `Cargo.toml`, keeping the Rust binary, file names, MSI, and release tag under one version source of truth.

The command above deliberately produces `uiAccess=false` while the public pipeline is unsigned.
A production accessibility artifact must set `NUMFLOW_UIACCESS=1` for the release build and then
Authenticode-sign both `numflow.exe` and `numflow-secure.exe` before WiX packaging. Sign the MSI
after packaging. Do not publish a `uiAccess=true` executable unless its signature validates and it
will be installed under `%ProgramFiles%\NumFlow`; Windows can refuse to start a UIAccess executable
that does not satisfy those trust requirements. The portable ZIP cannot provide protected-desktop
AT registration or trusted-location UIAccess.

## Signing

The current pipeline builds unsigned artifacts. Production code signing should be added when a Windows code-signing certificate and protected CI secret strategy are available. Do not embed private signing material in the repository.
