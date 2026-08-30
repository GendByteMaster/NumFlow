# Windows keyboard filter recovery gate

Status: required checklist for the first NumFlow keyboard filter install/load. The current
TestPackage applies only to the guarded disposable VirtualBox VM procedure. A dedicated test
machine or other VM requires a separate reviewed package and installation plan. The development
workstation is out of scope.

## Entry conditions

Do not perform the first install until all items are true:

- x64 Debug and Release builds pass;
- PREfast/Code Analysis and InfVerif pass;
- source review confirms 100% PASS_THROUGH behavior;
- the test VM has a snapshot taken while powered off;
- an alternate control path is available (VM console plus on-screen keyboard or remote console);
- Safe Mode boot has been rehearsed in that VM;
- the exact pre-install Keyboard class and device filter configuration has been exported;
- package removal commands have been prepared and checked against the VM package identity;
- crash dump collection and kernel debugging are configured;
- the VM snapshot restore has been tested.

Any failed item blocks installation.

## Evidence captured before install

Capture inside the VM:

- Windows build and architecture;
- VM snapshot identifier;
- `Keyboard` class `UpperFilters` value and relevant device-stack filter values;
- keyboard device instance IDs and driver stacks;
- NumFlow package INF name after staging;
- active test-signing and Driver Verifier state;
- a timestamped setup/API/driver log directory outside the disposable VM disk when possible.

## Recovery order

If keyboard input, PnP start, resume, reconnect, or shutdown fails:

1. Stop the test and keep the VM console attached.
2. Use the alternate control path; do not repeatedly reboot a failing stack.
3. Boot the VM into Safe Mode when normal boot cannot provide reliable input.
4. Disable/remove only the exact NumFlow test package and service identity.
5. Restore the captured pre-install filter configuration exactly, preserving order and unrelated
   filters.
6. Disable NumFlow-specific Driver Verifier settings if they prevent boot.
7. Reboot the VM and confirm every physical/test keyboard works before collecting final logs.
8. If recovery is uncertain or incomplete, power off and restore the known-good snapshot.

The exact removal commands must be filled with the package identity observed in the VM. This guide
is implemented by the guarded `driver/vm/remove-test-driver.ps1` helper, which discovers exactly
one NumFlow `oemNN.inf` identity and refuses ambiguous removal. It never removes system keyboard
drivers or unrelated filters.

## If the keyboard stops working

1. Keep the VirtualBox console open and stop generating input.
2. Use the on-screen keyboard, VM console controls, or the rehearsed alternate control path.
3. If Windows remains usable, run the guarded VM removal script and type back the exact published
   `oemNN.inf` name it prints.
4. Reboot the VM manually and confirm that the PS/2 keyboard stack is functional.
5. If the removal script cannot complete or the device stack is uncertain, power off the VM and
   restore `numflow-before-driver-install`.

Do not delete `kbdclass.sys`, `i8042prt.sys`, `keyboard.inf`, the Keyboard setup class, or any
unrelated filter/service.

## If normal boot is inaccessible

1. Enter the rehearsed Safe Mode path from the VirtualBox console or Windows Recovery Environment.
2. Use the alternate input path to remove only the exact NumFlow `oemNN.inf` package recorded at
   installation time.
3. If the package identity cannot be proven exactly, do not guess and do not remove a system
   package; restore the powered-off snapshot.
4. After recovery, boot normally and validate every keyboard before collecting logs.

## BSOD or boot-loop recovery

1. Do not keep cycling a crashing VM or enable additional Driver Verifier options.
2. Capture the bugcheck code and available dump path from the VM console.
3. Power off the VM and clone or preserve the failed virtual disk only if crash analysis is needed.
4. Restore `numflow-before-driver-install`; if TESTSIGNING or firmware changes are also suspect,
   restore `numflow-before-testsigning`.
5. Confirm the restored VM boots with normal keyboard input before attempting another package.

Snapshot restoration is the authoritative recovery path. Offline registry editing and manual
deletion of keyboard-stack binaries are deliberately excluded.

## PASS_THROUGH validation before ACTIVE work

With the filter installed only in the VM, verify:

- all normal keys and every NumPad make/break sequence reach Windows unchanged;
- Num Lock LED/state remains synchronized;
- no duplicate or missing packets under sustained input;
- USB/Bluetooth reconnect and multiple-keyboard use remain functional;
- sleep/resume, hibernate where supported, logon, lock/unlock, and shutdown remain functional;
- stopping/crashing any NumFlow user-mode process does not change keyboard behavior;
- removing the package restores the exact prior filter state;
- the same checks pass after at least three cold boots and three sleep/resume cycles.

Suppression, service handshake, and ACTIVE mode remain blocked until this gate passes.
