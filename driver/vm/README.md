# NumFlow PASS_THROUGH driver test in VirtualBox

These scripts are deliberately VM-only. Both scripts stop unless Windows identifies the machine as
VirtualBox and PowerShell is elevated. Never weaken or remove that guard for the development host.

## First install/load procedure

1. Create a disposable Windows 11 x64 VirtualBox VM with a PS/2 keyboard device.
2. Power off the VM and take a snapshot named `numflow-before-testsigning`.
3. Confirm that the VM console, on-screen keyboard, and Safe Mode recovery path work.
4. In an elevated VM terminal, run `bcdedit /set testsigning on`.
5. Power off the VM if needed, disable Secure Boot in the VM firmware, then boot Windows again.
6. Confirm that `bcdedit /enum {current}` shows `testsigning Yes`.
7. Power off the VM and take a second snapshot named `numflow-before-driver-install`.
8. Copy the five files from `driver/package/x64/Release/` into `C:\NumFlowTestPackage` in the VM.
9. Copy `driver/vm/common-vm.ps1`, `install-test-driver.ps1`, and
   `remove-test-driver.ps1` into `C:\NumFlowVmScripts` in the VM.
10. Open elevated PowerShell in `C:\NumFlowVmScripts` and run:

    ```powershell
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\install-test-driver.ps1 `
      -PackagePath 'C:\NumFlowTestPackage'
    ```

11. Reboot only when Windows or the test procedure requires it. Use `-Reboot` only when the VM
    console and both recovery snapshots are available.
12. Complete every PASS_THROUGH check in `docs/windows-driver-recovery.md` before any ACTIVE-mode
    development.
13. Remove the exact published NumFlow package with:

    ```powershell
    powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\remove-test-driver.ps1 `
      -PackagePath 'C:\NumFlowTestPackage'
    ```

    The script prints the `oemNN.inf` identity and requires typing it back. `-Force` skips only this
    prompt; it does not pass `pnputil /force`.
14. Reboot, confirm normal keyboard behavior, then restore `numflow-before-driver-install` (or the
    earlier snapshot if recovery is uncertain).

The install script imports only the public `NumFlowDriverTest.cer` into the VM's LocalMachine Root
and TrustedPublisher stores. The non-exportable private key stays in the development user's
CurrentUser certificate store and is never copied into the package.
