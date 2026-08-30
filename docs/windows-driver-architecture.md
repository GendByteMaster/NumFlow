# Windows Input Backend v2 architecture report

Status: Phase 0 architecture decision. This report authorizes preparation of the Phase 1
PASS-THROUGH skeleton; it does **not** authorize installation on the development workstation or
enable keyboard suppression.

The driver work is high risk because a faulty class filter can prevent keyboard input or crash
Windows. All early installation, Driver Verifier, power, reconnect, and removal testing must happen
in a disposable Windows 10/11 x64 VM with a tested recovery path.

The first signed PASS_THROUGH package intentionally narrows this architecture: it binds only to the
VirtualBox PS/2 compatible ID `*PNP0303` and uses a device-specific declarative `AddFilter` upper
filter. That lets the first load validate forwarding and recovery without making a class-wide host
change. Class-wide registration and HID hardware coverage remain production-design work.

## Decision summary

NumFlow will retain the existing user-mode backend and introduce a separate experimental driver
backend. The first driver is a class-wide KMDF keyboard filter placed immediately below
`KbdClass.sys` and above the keyboard port/mapper driver. It observes `KEYBOARD_INPUT_DATA` in the
class service callback, copies only classified NumPad packets into a bounded diagnostic queue, and
always forwards the original packet range unchanged.

For modern HID keyboards the input path is:

```text
USB / Bluetooth / I2C transport
  -> HidClass.sys
  -> KbdHid.sys
  -> NumFlow keyboard class filter (KMDF, one instance per keyboard stack)
  -> KbdClass.sys
  -> Windows input subsystem
```

For a legacy PS/2 keyboard the path is:

```text
i8042prt.sys
  -> NumFlow keyboard class filter
  -> KbdClass.sys
  -> Windows input subsystem
```

The production driver path will eventually be:

```text
Physical keyboard
  -> NumFlow keyboard filter
  -> protected per-device driver interface
  -> numflow-service.exe
  -> numflow-core policy, bindings, and motion
  -> protected virtual-mouse driver interface
  -> NumFlow VHF source driver
  -> Vhf.sys / HidClass.sys / MouHid.sys / MouClass.sys
  -> Windows pointer system
```

The existing path remains available:

```text
WH_KEYBOARD_LL -> numflow-core -> SendInput
```

## 1. Current NumFlow input path

The current application owns one process-wide `WH_KEYBOARD_LL` listener:

1. Slint creates the UI event loop.
2. `src/platform_input/windows.rs` removes winit's process-wide raw-keyboard registration. Normal
   window input remains intact, while global NumPad ownership stays with the low-level hook.
3. `src/runtime.rs` starts `KeyboardHook` with retry logic.
4. A dedicated owner thread in `crates/numflow-windows/src/hook.rs` installs
   `WH_KEYBOARD_LL`, owns its message loop, and serializes power/session/device recovery.
5. `keyboard_hook_proc` converts `KBDLLHOOKSTRUCT` into the current Rust
   `PhysicalKeyEvent { vk_code, scan_code, extended, state }`.
6. `map_numpad_key` classifies NumPad packets by scan code plus the extended flag. Top-row digits
   and the extended navigation cluster do not match.
7. When NumFlow is enabled, delivery to the bounded runtime queue succeeds, and Num Lock is off,
   the hook returns a nonzero result to suppress the original NumPad packet. A failed queue delivery
   falls through to `CallNextHookEx`, preserving keyboard input.
8. `KeyboardEventNormalizer`, shared bindings, `ControllerState`, and `MotionEngine` produce
   platform-independent actions and pointer effects.

Num Lock is a separate mode signal. Physical Num Lock input is observed and passed to Windows. A
tagged `SendInput` sequence is currently used only when lifecycle recovery must synchronize the
confirmed Num Lock state.

Current limitation relevant to v2: `NumpadKey` does not yet include NumPad Enter or Num Lock.
Num Lock must remain a policy event rather than a normal binding. NumPad Enter support is a later
core/API decision and is not part of the Phase 1 skeleton.

## 2. Current mouse output path

`RuntimeMachine<B: PointerBackend>` already separates core policy from pointer transport. The
Windows runtime currently instantiates it with `WindowsPointer`, whose implementation emits relative
movement and left/right/middle button events through `SendInput`.

The backend tracks only buttons injected by NumFlow. Disable, shutdown, lifecycle recovery, and
fault handling stop motion, reset pressed keyboard state, clear controller holds, and call
`release_all`. This fail-safe behavior remains mandatory for both pointer transports.

## 3. Where `WH_KEYBOARD_LL` remains

`WH_KEYBOARD_LL` remains the complete Standard backend and the default release path until the driver
backend passes VM, signing, lifecycle, multi-keyboard, and physical acceptance gates. It is not
removed or silently replaced.

The driver service and the GUI must never start the Standard listener concurrently for the same
active backend. Backend selection will be explicit and experimental. Failure to start or validate
the driver backend selects Standard or leaves physical input in PASS_THROUGH; it must never result
in two consumers suppressing the same event.

## 4. Where `SendInput` remains

`WindowsPointer` remains the Standard pointer implementation. Its `SendInput` behavior, UIPI
diagnostics, pressed-button tracking, and forced release logic remain available as the rollback
path.

The Virtual HID path will implement the same `PointerBackend` semantics through the service. Core
motion and acceleration continue to produce final `dx`/`dy`; the kernel does not calculate motion.
The Num Lock replay used by the Standard backend is independent of virtual mouse output and is not
removed during the keyboard PoC.

## 5. Proposed keyboard filter architecture

The selected design is a **class-wide upper filter registration positioned below `KbdClass`**. In
Microsoft's Kbfiltr terminology, the NumFlow service name is ordered before `KbdClass` in the
Keyboard setup class `UpperFilters` list. The filter participates in each keyboard device stack and
intercepts `IOCTL_INTERNAL_KEYBOARD_CONNECT` to substitute a filter service callback while retaining
the original `CONNECT_DATA` callback and context.

The filter service callback:

1. receives the original `KEYBOARD_INPUT_DATA` range;
2. classifies NumPad packets from `MakeCode` and `Flags` (`KEY_MAKE`, `KEY_BREAK`, `KEY_E0`,
   `KEY_E1`);
3. copies matching diagnostic events to a fixed-capacity, nonpaged per-instance ring;
4. updates counters without per-event production logging;
5. calls the saved KbdClass service callback with the original range and original ordering.

During Phases 1–7 it never edits packets, changes the packet count, consumes packets, or changes
`InputDataConsumed`. The callback runs at `DISPATCH_LEVEL`, so it performs no blocking operation,
file I/O, user-mode call, unbounded allocation, or unbounded wait. A short framework spin lock may
protect the preallocated ring; service request completion and verbose tracing must stay outside the
hot callback where practical.

### Why this level

- It observes `KEYBOARD_INPUT_DATA` after HID usages have been mapped into keyboard scan-code
  packets, matching NumFlow's existing physical scan-code classifier.
- It is above the transport boundary, so the same callback contract works for `KbdHid` devices and
  for the legacy `i8042prt` path.
- A filter instance belongs to a particular keyboard device stack, preserving per-device context
  needed for hot-plug and future device selection.
- It does not replace `KbdHid`, `KbdClass`, `HidClass`, or a transport minidriver.
- It avoids the Microsoft-discouraged position between `HidClass` and HID transport minidrivers.

### Why not copy all of Kbfiltr

Kbfiltr is an upper device filter sample for a PS/2 keyboard. Its
`IOCTL_INTERNAL_I8042_HOOK_KEYBOARD`, initialization, and ISR hooks are specific to `i8042prt` and
must not enter the NumFlow production design. NumFlow reuses only the documented patterns for:

- `IOCTL_INTERNAL_KEYBOARD_CONNECT` interception;
- a class service callback that forwards to KbdClass;
- KMDF filter initialization;
- exposing a separate raw PDO/interface because the keyboard collection itself is exclusive.

### Installation boundary

The application runtime never edits `UpperFilters`. A dedicated elevated installer must install the
signed package/service and atomically preserve the existing multi-string filter order. Uninstall
must restore the previous class configuration. The exact install/rollback procedure is blocked on
the recovery document and VM validation.

## 6. USB, Bluetooth, embedded, and multiple keyboards

Microsoft documents `HidClass.sys` as the transport-independent layer used by USB, Bluetooth, and
other HID transports. `KbdHid.sys` maps keyboard HID usages to scan codes for `KbdClass.sys`.
Because the selected filter sits between `KbdHid` and `KbdClass`, it is independent of the HID
transport and therefore covers USB and Bluetooth keyboard top-level collections. A class-wide
registration also attaches to laptop/PS/2 keyboard stacks that connect through KbdClass.

Each keyboard stack gets its own filter context, ring, counters, PnP/power lifecycle, raw PDO, and
device interface. The service enumerates all `GUID_DEVINTERFACE_NUMFLOW_KBD_V1` instances and reacts
to interface arrival/removal.

The protocol carries an opaque 64-bit runtime `device_id`. The service assigns it from the opened
interface identity for the current service epoch. Persistent user selection is stored separately
using the PnP device instance ID and, when available, container ID. Microsoft documents device
instance IDs as persistent across restarts, but identical devices without a serial/container ID can
remain port-dependent. NumFlow must report that limitation instead of claiming universal physical
identity across reconnects or ports.

`KEYBOARD_INPUT_DATA.UnitId` is retained as diagnostic metadata but is not treated as the persistent
identity by itself.

## 7. Driver-to-service communication and protocol

Each filter instance exposes a separate raw PDO/device interface. This follows the official Kbfiltr
communication pattern and avoids trying to open the exclusive keyboard collection from user mode.
The GUI cannot open these endpoints.

Initial interface operations:

- `IOCTL_NUMFLOW_GET_CAPABILITIES` — protocol range, driver build, state, queue capacity, and device
  metadata;
- `IOCTL_NUMFLOW_READ_EVENTS` — one overlapped, cancel-safe request returning a bounded event batch;
- `IOCTL_NUMFLOW_GET_DIAGNOSTICS` — counters and last state-transition reason;
- later phases only: `IOCTL_NUMFLOW_BEGIN_SESSION`, `IOCTL_NUMFLOW_HEARTBEAT`, and
  `IOCTL_NUMFLOW_SET_MODE`.

The interface ACL is declared in the driver package and permits only `SYSTEM` and the dedicated
NumFlow service identity required by the final service account design. There is one authenticated
service client per endpoint. Arbitrary desktop applications, including the GUI, receive no direct
driver access.

### Stable binary framing

No compiler-layout C or Rust structure crosses the boundary. Every message is serialized and
validated field-by-field in little-endian order.

Protocol v1 uses a fixed 24-byte frame header:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 4 | ASCII magic `NFV2` |
| 4 | 2 | protocol version (`1`) |
| 6 | 2 | message type |
| 8 | 2 | header size (`24`) |
| 10 | 2 | flags; unknown required flags reject the frame |
| 12 | 4 | payload size, bounded by the IOCTL contract |
| 16 | 8 | sequence ID |

An event batch starts with a count and contains fixed-width v1 physical-key records:

| Field | Type | Meaning |
| --- | --- | --- |
| `device_id` | `u64` | opaque service-epoch keyboard identity |
| `timestamp_100ns` | `u64` | monotonic driver timestamp, not wall time |
| `packet_sequence` | `u64` | per-filter monotonic packet sequence |
| `make_code` | `u16` | physical scan code from `KEYBOARD_INPUT_DATA` |
| `packet_flags` | `u16` | explicitly allowed make/break/E0/E1 bits |
| `key_state` | `u8` | normalized pressed/released value |
| `classification` | `u8` | NumPad classification or unknown |
| `reserved` | `u16` | must be zero |

All lengths, counts, enum values, reserved fields, sequence ordering, and version ranges are checked
before events reach NumFlow Core. A protocol mismatch closes the endpoint, records a diagnostic, and
leaves the driver in PASS_THROUGH.

The service keeps overlapped reads outstanding so the driver can return batches rather than make a
kernel transition per packet. Pending requests are cancel-safe and purged on file close/removal.

## 8. Queue and overflow behavior

Each instance uses a fixed-capacity queue allocated during device initialization. Capacity becomes a
versioned capability and is tuned in VM stress tests; no runtime input can grow kernel memory.

On overflow:

1. the original keyboard packet is forwarded unchanged;
2. `queue_drops` increments;
3. the oldest/newest drop policy is explicit in the implementation (initial recommendation: drop
   the newest telemetry record to preserve already ordered records);
4. if suppression exists in a later phase, suppression is atomically disabled before a packet can
   be lost to user mode;
5. the endpoint reports DEGRADED diagnostics while its behavioral mode is PASS_THROUGH.

Keyboard responsiveness and physical key delivery always outrank telemetry completeness.

## 9. Fail-safe and heartbeat

The kernel state machine is intentionally smaller than the application lifecycle state machine:

```text
PASS_THROUGH --validated service session + runtime ready--> ACTIVE
ACTIVE --disable request / timeout / close / D0Exit / error--> PASS_THROUGH
any state --health warning--> DEGRADED diagnostics + PASS_THROUGH behavior
```

Rules:

- boot, device start, D0 entry, reconnect, protocol mismatch, service absence, and service close all
  mean PASS_THROUGH;
- Phases 1–7 never suppress, so ACTIVE is diagnostic-only until the separate suppression phase;
- after suppression exists, it is permitted only while the service session, protocol, heartbeat,
  runtime readiness, NumFlow enabled state, and queue health are all valid;
- the hot callback reads a single atomic suppression gate and never waits for user mode;
- D0 exit and device removal clear the gate before queue teardown;
- reconnect creates a fresh handshake and never inherits ACTIVE.

The initial heartbeat design is one service heartbeat every 250 ms with a 1,000 ms expiry. A single
WDF timer checks the monotonic deadline only while a session is active. False expiry is safe because
it restores ordinary keyboard behavior. These values are provisional until Phase 6 scheduling,
resume, and stress tests establish the final documented constants.

## 10. GUI/service boundary

`numflow-service.exe` owns the driver endpoints, protocol validation, driver/backend health,
bindings, NumFlow Core, and later the virtual-mouse endpoint. The GUI uses a local versioned named
pipe only for configuration and status.

The GUI does not send arbitrary scan-code or mouse-report commands. This restriction prevents the
privileged service from becoming a general input-injection broker. The pipe uses bounded frames,
request IDs, strict parsing, and an ACL limited to the intended interactive user and service. The
service-to-driver interfaces use a narrower service-only ACL.

Session lock, secure desktop, service stop, update, and shutdown force PASS_THROUGH and stop virtual
mouse reports. The design must not attempt to bypass UAC, credential UI, the lock screen, or Secure
Desktop.

## 11. Virtual HID Mouse design

The virtual mouse is a separate root-enumerated KMDF HID source driver, not part of the keyboard
filter callback path. It links `Vhfkm.lib`, creates a VHF device, and declares `Vhf.sys` as the lower
filter in its device stack. VHF enumerates the HID child consumed by the normal Windows HID mouse
path.

The minimal report descriptor exposes one relative mouse application collection with:

- three buttons (left, right, middle);
- signed relative X;
- signed relative Y;
- no wheel or additional buttons in the first milestone.

The service sends semantic `MoveRelative`, `ButtonDown`, and `ButtonUp` operations over the protected
driver interface. The driver maintains the current three-bit button bitmap and submits complete HID
input reports. It does not calculate acceleration, bindings, mode, or timers.

VHF has its own flow control: after submitting a read report, the source waits until VHF is ready
for the next report rather than adding a second unbounded buffer. Movement may be coalesced within a
strict bound, but button transitions remain ordered.

On service close, backend disable, update, shutdown, and safe power-down paths, the driver submits a
zero-button report when VHF can accept it and clears its internal bitmap. On D0 entry it starts from
a zero-button state before accepting commands. The service independently clears Core hold/movement
state. If the device is already unavailable and a release report cannot be delivered, re-enumeration
still begins from the all-released report state.

`SendInput` is not used by this backend, but remains the complete Standard fallback.

## 12. WDK references and samples

Primary Microsoft references:

- [Keyboard and mouse HID client drivers](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/keyboard-and-mouse-hid-client-drivers)
  documents `KbdHid`, `KbdClass`, HID transports, permitted filter positions, and the recommendation
  to use WDF.
- [HID architecture](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/hid-architecture)
  documents the transport-independent `HidClass` boundary and exclusive keyboard/mouse collections.
- [Kbfiltr](https://learn.microsoft.com/en-us/samples/microsoft/windows-driver-samples/keyboard-input-wdf-filter-driver-kbfiltr/)
  is used only for the KbdClass connect callback, class-filter ordering, and raw PDO patterns; its
  PS/2 ISR hooks are not a portable design.
- [IOCTL_INTERNAL_KEYBOARD_CONNECT](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/kbdmou/ni-kbdmou-ioctl_internal_keyboard_connect)
  defines the KbdClass connection interception contract.
- [Keyboard class service callback](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/kbdmou/nc-kbdmou-pservice_callback_routine)
  documents packet ranges and the `DISPATCH_LEVEL` execution requirement.
- [KEYBOARD_INPUT_DATA](https://learn.microsoft.com/en-us/windows/win32/api/ntddkbd/ns-ntddkbd-keyboard_input_data)
  defines `UnitId`, `MakeCode`, make/break, and E0/E1 packet flags.
- [Installing a filter driver](https://learn.microsoft.com/en-us/windows-hardware/drivers/install/installing-a-filter-driver)
  documents device/class filter installation and the installer boundary.
- [Using device interfaces](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/using-device-interfaces)
  and [controlling device access](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/controlling-device-access-in-kmdf-drivers)
  define the service endpoint and ACL model.
- [VHF HID source driver](https://learn.microsoft.com/en-us/windows-hardware/drivers/hid/virtual-hid-framework--vhf-)
  defines the KMDF source, `Vhf.sys` lower filter, report submission, flow control, and teardown.
- [KMDF PnP/power callback mapping](https://learn.microsoft.com/en-us/windows-hardware/drivers/wdf/wdm-irps-and-kmdf-event-callback-functions)
  defines D0, stop, surprise-removal, and teardown ordering.
- [Driver security checklist](https://learn.microsoft.com/en-us/windows-hardware/drivers/driversecurity/driver-security-checklist)
  and [Driver Verifier](https://learn.microsoft.com/en-us/windows-hardware/drivers/devtest/driver-verifier)
  define security and test-machine expectations.
- [Driver signing options](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings)
  distinguishes development/preproduction/attestation paths from the recommended HLK production
  path.

No third-party GitHub driver is an architectural source of truth. Source copied or adapted from the
official Microsoft samples must be reviewed against the current WDK documentation and reduced to
the NumFlow requirements.

## 13. Risks, rollback, and Phase 1 gate

### Principal risks

- incorrect `UpperFilters` ordering can prevent keyboard stacks from starting;
- a callback lifetime, IRQL, buffer-range, or synchronization bug can cause a BSOD;
- teardown races across hot-plug, D0 exit, pending reads, and the raw PDO can access freed memory;
- suppression plus queue/service loss can drop physical keys;
- a permissive device or pipe ACL can create a privileged input-injection boundary;
- incorrect VHF flow control or button state can duplicate movement or leave a button held;
- signing/install mistakes can strand a non-starting filter package on a machine.

### Rollback boundary

Before the first installation, `docs/windows-driver-recovery.md` must document and verify in the VM:

- a snapshot/checkpoint;
- Safe Mode access without the NumFlow filter;
- removal of the package/service with `pnputil` or the supported installer path;
- restoration of the exact previous Keyboard class `UpperFilters` multi-string;
- keyboard-stack restart/reboot behavior;
- collection of setup logs, driver traces, crash dumps, and verifier state.

No filter is installed on the primary workstation before that procedure passes.

### Phase 1 acceptance gate

Phase 1 may create only a buildable KMDF skeleton and packaging metadata. It succeeds when:

- the solution builds with the pinned Visual Studio/WDK toolchain on x64;
- DriverEntry and per-device add/remove callbacks contain no input modification;
- the device is marked as a filter and forwarding behavior is explicit;
- the INF is inspectable but is not installed on the host;
- PREfast/Code Analysis and InfVerif results are recorded;
- no application/core/UI behavior changes;
- Standard `WH_KEYBOARD_LL + SendInput` tests continue to pass.

Phase 2 owns the first class-service callback and must remain 100% PASS_THROUGH. Phase 3 owns the raw
PDO and Rust test client. Suppression is prohibited until the later heartbeat/fail-safe phases pass
their VM gates.
