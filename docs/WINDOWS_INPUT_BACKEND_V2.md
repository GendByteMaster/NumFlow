# Windows Input Backend v2

Статус: исходное архитектурное ТЗ для Phase 0. Перед реализацией требуется подготовить короткий architecture report и подтвердить выбранную архитектуру по актуальной официальной документации Microsoft WDK.

Нужно спроектировать и реализовать новый Windows Input Backend v2 для NumFlow на базе собственного Keyboard Filter Driver и Virtual HID Mouse.

Цель — уйти от зависимости от `WH_KEYBOARD_LL + SendInput` как основного input/output пути и приблизить архитектуру NumFlow к системному Windows input stack.

Текущая user-mode реализация остаётся fallback backend и не должна удаляться до тех пор, пока driver backend не пройдёт полноценное тестирование.

Перед изменениями обязательно изучить актуальную официальную документацию Microsoft WDK, KMDF, HID, Keyboard/Mouse class drivers, Kbfiltr и Virtual HID Framework.

Использовать официальные Microsoft sources как основной reference:

- Keyboard Input WDF Filter Driver / Kbfiltr. Microsoft указывает, что sample является upper device filter для PS/2 keyboard и работает между `KbdClass` и `i8042prt`; его нельзя слепо копировать как универсальную USB/Bluetooth реализацию.
- Windows HID keyboard/mouse stack: `KBDHID.sys`, `KBDCLASS.sys`, `MOUHID.sys`, `MOUCLASS.sys`.
- Virtual HID Framework (VHF), позволяющий KMDF/WDM HID source driver создавать виртуальные HID devices и передавать HID input reports Windows.
- Windows HID architecture.
- Windows driver signing requirements для публичного распространения.

Не использовать случайные GitHub driver examples как архитектурный source of truth, если поведение не подтверждено Microsoft WDK documentation.

## 1. Целевая архитектура

Нужно прийти к архитектуре:

```text
Physical Keyboard
↓
Windows HID / Keyboard Stack
↓
NumFlow Keyboard Filter Driver
↓
physical NumPad events
↓
NumFlow Windows Service
↓
NumFlow Core
↓
bindings / acceleration / modes
↓
NumFlow Virtual HID Mouse
↓
Windows HID / Mouse Stack
↓
Windows Pointer System
```

При этом GUI остаётся отдельным процессом:

```text
NumFlow GUI
↕ IPC
NumFlow Service
↕ Device IOCTL / driver interface
NumFlow Driver
```

GUI не должен напрямую содержать kernel-level input logic.

## 2. Не переписывать весь NumFlow сразу

Сохранить существующую архитектуру backend abstraction.

Должно существовать минимум два Windows backend:

- `WindowsUserModeBackend`;
- `WindowsDriverBackend`.

Standard backend:

- `WH_KEYBOARD_LL`;
- текущая keyboard logic;
- текущий mouse output.

Driver backend:

- Keyboard Filter Driver;
- service IPC;
- Virtual HID Mouse.

Не удалять рабочий user-mode backend.

Driver backend сначала должен быть experimental/opt-in.

## 3. Первый milestone — Keyboard Filter Driver

Не начинать сразу с interception клавиш.

Первый этап должен быть полностью PASS-THROUGH.

Driver должен:

1. корректно загрузиться;
2. подключиться к keyboard stack;
3. видеть keyboard packets;
4. определять NumPad physical events;
5. отправлять копию этих событий user-mode service;
6. всегда пропускать исходный keyboard input дальше без изменения.

```text
Keyboard packet
↓
NumFlow filter
├── copy → NumFlow Service
└── original → Windows
```

На первом этапе нельзя:

- блокировать NumPad;
- изменять scan codes;
- добавлять synthetic keyboard input;
- перехватывать всю клавиатуру ради NumPad;
- менять поведение Windows keyboard stack.

## 4. Не копировать Kbfiltr вслепую

Официальный Microsoft Kbfiltr sample ориентирован на PS/2 stack и работает между `KbdClass` и `i8042prt`.

Современные USB/Bluetooth keyboards проходят через HID stack:

```text
HID transport
→ HIDCLASS
→ KBDHID
→ KBDCLASS
```

Windows использует `KBDHID.sys` как HID keyboard mapper и `KBDCLASS.sys` как keyboard class driver.

Поэтому сначала определить правильную filter architecture для современных USB/Bluetooth клавиатур.

Нужно явно документировать:

- где именно располагается NumFlow filter;
- device filter это или class filter;
- почему выбран именно этот уровень;
- поддерживает ли он USB;
- поддерживает ли Bluetooth HID;
- поддерживает ли несколько клавиатур;
- какие ограничения есть.

Не изменять системный `UpperFilters` registry вручную из application runtime.

Installation должна выполняться через нормальный driver package/INF.

## 5. Driver implementation

Предпочтительно реализовать первый production driver через:

- C или C++;
- KMDF;
- Windows Driver Kit.

Основное приложение NumFlow остаётся Rust.

Не переносить kernel driver на Rust только ради единого языка, если это увеличивает риск или усложняет WDK integration.

Ориентировочная структура:

```text
driver/
├── numflow-kbd-filter/
│   ├── driver.c
│   ├── device.c
│   ├── filter.c
│   ├── queue.c
│   ├── communication.c
│   ├── power.c
│   ├── numflow_kbd.h
│   ├── numflow-kbd.inf
│   └── numflow-kbd.vcxproj
│
└── README.md
```

Точную структуру подобрать после анализа WDK architecture.

## 6. Минимум логики в kernel

Kernel driver не должен содержать:

- pointer acceleration;
- NumFlow modes;
- UI state;
- TOML config;
- bindings;
- HUD logic;
- complex timers;
- business logic.

Kernel должен делать только:

- keyboard packet observation/filtering;
- NumPad classification;
- device identification;
- controlled event delivery;
- optional suppression после отдельного milestone;
- power/PnP lifecycle;
- communication with service.

Вся политика остаётся в user mode.

## 7. Driver → Service protocol

Создать минимальный стабильный binary protocol.

Например, `PhysicalKeyEvent` с полями:

- `protocol_version`;
- `device_id`;
- `scan_code`;
- `flags`;
- `key_state`;
- `extended`;
- `timestamp`;
- `sequence_id`.

Не передавать C structs напрямую без фиксированного ABI/versioning.

Добавить protocol version, например `NUMFLOW_DRIVER_PROTOCOL_V1`.

Service должен уметь определить несовместимую версию driver protocol и безопасно отключить driver backend.

## 8. NumPad classification

Поддержать физические события:

- NumPad 0;
- NumPad 1;
- NumPad 2;
- NumPad 3;
- NumPad 4;
- NumPad 5;
- NumPad 6;
- NumPad 7;
- NumPad 8;
- NumPad 9;
- NumPad +;
- NumPad -;
- NumPad *;
- NumPad /;
- NumPad Decimal/Delete;
- NumPad Enter;
- Num Lock.

Очень важно правильно различать:

- NumPad keys;
- navigation keys;
- extended scan codes;
- обычные цифровые клавиши верхнего ряда.

Не полагаться только на virtual key, если на выбранном уровне stack доступна более надёжная physical information.

## 9. Multiple keyboards

Driver architecture должна корректно работать с:

- одной USB keyboard;
- несколькими keyboard devices;
- Bluetooth keyboard;
- встроенной laptop keyboard;
- reconnect;
- hot-plug.

Нужно иметь стабильный runtime `device_id`.

Не привязывать NumFlow навсегда к случайному handle, который изменится после reconnect.

Подготовить возможность в будущем выбирать конкретную клавиатуру NumFlow.

## 10. Service layer

Создать отдельный Windows service или equivalent privileged background component: `numflow-service.exe`.

Задачи:

- открыть driver device interface;
- читать `PhysicalKeyEvent`;
- выполнять protocol validation;
- передавать события NumFlow Core;
- heartbeat;
- driver mode coordination;
- recovery;
- logging;
- IPC с GUI.

GUI не должен быть required для работы input backend.

После закрытия главного окна NumFlow service/runtime должен продолжать работать согласно существующему background behavior.

## 11. IPC GUI ↔ Service

Создать локальный защищённый IPC.

Варианты:

- Windows Named Pipe;
- другой подходящий local Windows IPC.

Нужно обеспечить:

- только local machine;
- authentication/ACL;
- protocol version;
- request IDs;
- bounded message size;
- защиту от malformed messages.

GUI получает:

- driver status;
- connected devices;
- active backend;
- health;
- update/config state.

GUI отправляет:

- configuration changes;
- backend selection;
- enable/disable NumFlow;
- bindings;
- speed/acceleration settings.

## 12. Fail-safe — обязательное требование

Keyboard driver по умолчанию должен работать в режиме `PASS_THROUGH`.

Нельзя допустить ситуацию, когда service или GUI упали, произошёл Rust panic, выполняется update или потерян IPC, и NumPad или клавиатура перестают работать.

Driver states:

- `PASS_THROUGH`;
- `ACTIVE`;
- `DEGRADED`.

По умолчанию: `PASS_THROUGH`.

После успешного handshake с service:

```text
PASS_THROUGH → ACTIVE
```

При проблеме:

```text
ACTIVE → PASS_THROUGH
```

Переход должен происходить автоматически.

## 13. Heartbeat

Добавить service heartbeat.

Service периодически подтверждает:

- protocol alive;
- NumFlow Core alive;
- interception safe.

Если heartbeat отсутствует в течение bounded timeout, driver автоматически возвращается в `PASS_THROUGH`.

Не использовать kernel timer logic сложнее необходимого.

Timeout должен быть безопасным и документированным.

## 14. Interception включать только после PoC

После того как PASS-THROUGH driver стабильно работает, добавить отдельный milestone: Optional NumPad suppression.

Только тогда driver сможет:

```text
NumPad event
↓
NumFlow ACTIVE
├── suppress from normal Windows keyboard path
└── send to NumFlow Service
```

Suppression разрешена только если:

- service connected;
- heartbeat valid;
- protocol compatible;
- runtime ready;
- NumFlow enabled.

При любой ошибке: `PASS_THROUGH`.

## 15. Num Lock

Не переносить NumFlow ON/OFF policy полностью в kernel.

Driver должен передавать physical Num Lock event.

NumFlow Service/Core остаётся source of truth для policy.

Сохранить существующую семантику проекта, если она подтверждена текущим кодом.

Не использовать registry hacks для изменения Num Lock.

## 16. Power / Sleep / Resume

Driver backend должен корректно переживать:

- Sleep;
- Hibernate;
- Modern Standby, если применимо;
- Resume;
- Session Lock;
- Session Unlock;
- device D-state transitions;
- device reconnect;
- surprise remove.

Kernel power lifecycle и Windows session lifecycle не смешивать.

Driver должен обрабатывать соответствующие KMDF PnP/power callbacks.

После D0 resume:

- восстановить device state;
- не создавать duplicate communication endpoint;
- не создавать duplicate filter;
- вернуть `PASS_THROUGH` до успешного service handshake;
- затем разрешить `ACTIVE`.

Не использовать пользовательский `desktop-switch` как substitute для power event.

## 17. Virtual HID Mouse — второй основной milestone

После стабильного Keyboard Filter Driver реализовать Virtual HID Mouse.

Использовать Microsoft Virtual HID Framework (VHF).

VHF позволяет KMDF/WDM HID source driver создавать виртуальный HID device и отправлять Windows HID input reports.

Целевая схема:

```text
NumFlow Core
↓
mouse command
↓
NumFlow Service
↓
driver interface
↓
Virtual HID Mouse
↓
Windows HID stack
↓
MOUHID / MOUCLASS
↓
Pointer
```

Windows использует `MOUHID.sys` для преобразования HID usages в mouse X/Y/buttons/wheel, а `MOUCLASS.sys` предоставляет системный mouse class path.

## 18. Virtual HID descriptor

Создать минимальный HID Report Descriptor для обычной относительной мыши.

Нужно поддержать минимум:

- relative X;
- relative Y;
- left button;
- right button;
- middle button.

Позже можно добавить:

- wheel;
- additional buttons.

Не добавлять capabilities, которые NumFlow не использует.

## 19. Mouse reports

Service/Core должен формировать semantic commands:

- `MoveRelative(dx, dy)`;
- `ButtonDown(Left)`;
- `ButtonUp(Left)`;
- `ButtonDown(Right)`;
- `ButtonUp(Right)`;
- `ButtonDown(Middle)`;
- `ButtonUp(Middle)`.

Driver преобразует это в HID report.

Не переносить acceleration calculation в kernel.

Acceleration остаётся в NumFlow Core (`Normal`, `Precision`, `Fast`). Core вычисляет `dx / dy`, и driver только доставляет готовый относительный HID movement report.

## 20. Fail-safe для mouse buttons

Это критично: нельзя допустить stuck mouse button.

При service disconnect, driver mode disable, process crash, update, shutdown или suspend отправить release state для всех виртуальных mouse buttons, если это безопасно на данном lifecycle этапе.

Core также должен очищать:

- `mouse_hold`;
- movement state;
- pressed NumPad state.

## 21. `SendInput` оставить fallback

Не удалять `SendInput` сразу.

Backend abstraction:

- `MouseBackend::SendInput`;
- `MouseBackend::VirtualHid`.

Driver mode:

- Keyboard = FilterDriver;
- Mouse = VirtualHid.

Fallback:

- Keyboard = `WH_KEYBOARD_LL`;
- Mouse = `SendInput`.

Это позволит сравнивать behavior и быстро отключать driver backend при проблемах.

## 22. Backend selection

Добавить в Advanced/Settings:

```text
Input Backend

Standard
No driver required

NumFlow Driver
Enhanced Windows input backend
```

Пока driver experimental:

```text
NumFlow Driver
Experimental
```

Не переключать пользователя автоматически на kernel driver без подтверждения.

## 23. Installation

Создать полноценный driver package:

- `.sys`;
- `.inf`;
- `.cat`;
- required metadata.

Не копировать `.sys` вручную в system folders.

Installer должен:

1. проверить OS/architecture;
2. установить driver package;
3. установить service;
4. зарегистрировать required device/filter configuration;
5. проверить successful load;
6. сохранить rollback information.

Uninstaller должен корректно удалить всё.

## 24. Driver signing

Разделить Development и Production signing.

Development:

- WDK test signing;
- test certificate;
- test machine/VM.

Production:

- Microsoft-supported driver signing pipeline;
- Hardware Dev Center;
- required EV/code-signing credentials;
- signed catalog/package.

Microsoft требует signing новых kernel-mode drivers для современных Windows через Hardware Developer Center pipeline.

Не хранить signing private keys в Git.

## 25. Очень важно — test environment

Не проводить ранние kernel-driver эксперименты только на основной рабочей Windows.

Подготовить:

- VM с Windows либо отдельную test machine;
- kernel debugging;
- crash dump collection;
- recovery instructions.

Перед первой installation написать recovery guide:

- Safe Mode;
- disable/remove driver;
- restore filter configuration;
- remove service;
- recover keyboard stack.

Не устанавливать class filter на основной компьютер до успешного VM validation.

## 26. BSOD safety

Driver должен быть минимальным.

Запрещено в callbacks:

- blocking network operations;
- file I/O без строгой необходимости;
- unbounded waits;
- complex allocation paths;
- user-mode assumptions;
- panic-like termination.

Проверить:

- IRQL requirements;
- memory lifetime;
- synchronization;
- race conditions;
- device removal;
- invalid pointers;
- queue lifetime.

Использовать WDF primitives вместо ручной WDM реализации там, где возможно.

## 27. Logging

Kernel logging:

- driver loaded;
- device attached;
- keyboard connected;
- service connected;
- `ACTIVE`/`PASS_THROUGH` transitions;
- D0Entry;
- D0Exit;
- device remove;
- heartbeat timeout;
- protocol mismatch.

Не логировать каждое keyboard event в production.

Для debug build разрешить verbose packet tracing через отдельный flag.

User-mode structured logging:

```text
Driver:
status=active
protocol=v1
devices=2

Driver:
sleep entered

Driver:
resume complete
mode=pass-through

Driver:
service handshake complete
mode=active
```

## 28. Metrics

Service должен иметь diagnostics counters:

- `driver_events_received`;
- `numpad_events_received`;
- `numpad_events_dispatched`;
- `numpad_events_suppressed`;
- `driver_reconnects`;
- `heartbeat_failures`;
- `power_resumes`;
- `virtual_mouse_reports`;
- `mouse_button_forced_releases`;
- `protocol_errors`;
- `queue_drops`.

Это позволит сравнить новый backend с текущим `WH_KEYBOARD_LL`.

## 29. Queue design

Kernel → service queue должна быть bounded.

Нельзя позволить user-mode stall бесконечно накапливать keyboard packets.

При overflow:

- keyboard input Windows не блокировать;
- перейти в `PASS_THROUGH` при необходимости;
- увеличить drop counter;
- логировать health warning.

Keyboard responsiveness важнее telemetry.

## 30. Security boundary

Не использовать driver для обхода:

- Secure Desktop;
- Windows Lock Screen security;
- credential UI;
- UAC security boundary;
- других Windows security controls.

Цель driver backend:

- надёжный NumPad input;
- стабильный Sleep/Resume;
- корректный elevated-app compatibility там, где это естественно обеспечивает HID path;
- virtual HID mouse output.

Не реализовывать security bypass.

## 31. Tests — driver unit/static

Добавить тестируемую отдельно pure logic часть:

- scan code classification;
- NumPad mapping identification;
- protocol serialization;
- protocol validation;
- device ID handling;
- heartbeat state machine;
- `ACTIVE`/`PASS_THROUGH` transition;
- queue overflow policy.

Где возможно вынести эту логику из kernel callbacks в тестируемые функции.

## 32. Integration tests

Keyboard:

- NumPad 0–9;
- Add;
- Subtract;
- Decimal;
- Enter;
- NumLock.

Negative:

- top-row digits не считаются NumPad;
- normal keys проходят untouched.

Multiple devices:

- USB keyboard;
- second keyboard;
- reconnect.

Lifecycle:

- service disconnect;
- service reconnect;
- driver restart;
- Sleep/Resume;
- Lock/Unlock.

Mouse:

- move;
- left click;
- right click;
- middle click;
- hold/release;
- forced release after service crash.

## 33. Stress tests

Проверить:

- удержание NumPad key 60 секунд;
- быстрые повторные keydown/keyup;
- simultaneous diagonal movement;
- button hold + movement;
- multiple keyboards;
- USB reconnect × 20;
- Sleep/Resume × 10;
- Lock/Unlock × 20;
- service restart × 20;
- GUI restart при работающем service;
- driver `ACTIVE` → service crash.

Не должно быть:

- BSOD;
- keyboard freeze;
- stuck button;
- duplicate input;
- duplicate driver callbacks;
- unbounded queue growth.

## 34. Реальный критерий исправления текущего бага

После реализации driver backend проверить текущий проблемный сценарий:

```text
NumFlow Driver = Active

Num Lock = состояние, необходимое для NumFlow

NumPad работает
→ Sleep
→ Wake
→ Unlock
→ сразу нажать NumPad
```

NumPad должен работать сразу без:

- toggle Num Lock;
- restart NumFlow;
- restart service;
- открытия GUI;
- повторной установки hook.

Повторить минимум 10 раз.

## 35. Task Manager

Отдельно проверить:

- обычный Task Manager;
- elevated Task Manager;
- NumPad movement;
- click;
- hold/release.

Сравнить Standard backend и Driver + Virtual HID backend.

Не утверждать, что Keyboard Filter Driver исправляет output problem, если input приходит нормально, а проблема находится в mouse injection.

Именно поэтому тестировать input и output counters отдельно.

## 36. Migration strategy

Не удалять текущий lifecycle refactor.

Он остаётся важным для:

- Standard backend;
- service lifecycle;
- GUI;
- fallback;
- non-Windows platforms.

Driver не является заменой всей state machine приложения.

Он заменяет только низкоуровневый Windows input/output transport.

## 37. Cross-platform boundary

Весь driver code должен находиться за Windows-specific abstraction.

Не загрязнять core директивами `#[cfg(windows)]` по всему проекту.

Предпочтительно:

```text
numflow-core
↑
backend trait
↑
numflow-windows
├── user_mode
└── driver
```

Linux/macOS не должны зависеть от WDK или driver crates.

## 38. Implementation phases

Не пытаться реализовать всё одним giant diff.

Разделить работу минимум на:

- Phase 0 — Architecture audit + Microsoft WDK research.
- Phase 1 — KMDF keyboard filter skeleton.
- Phase 2 — 100% PASS_THROUGH keyboard filter.
- Phase 3 — Driver ↔ Rust test client communication.
- Phase 4 — NumPad physical event detection.
- Phase 5 — NumFlow Service.
- Phase 6 — Heartbeat + fail-safe.
- Phase 7 — Sleep/Resume + PnP robustness.
- Phase 8 — Optional NumPad suppression.
- Phase 9 — Virtual HID Mouse via VHF.
- Phase 10 — NumFlow Core integration.
- Phase 11 — Installer/uninstaller.
- Phase 12 — Test signing / VM validation.
- Phase 13 — Production signing preparation.

После каждой фазы должен быть отдельный validation gate.

## 39. Phase 1 success criterion

Первый PR НЕ должен сразу управлять мышью.

Первый настоящий milestone:

```text
Physical NumPad
→ Windows keyboard stack
→ NumFlow Keyboard Filter
→ Rust test client
```

При этом Windows продолжает получать исходные клавиши без изменений.

После этого проверить:

- Sleep/Resume;
- reconnect;
- multiple keyboards.

Только после стабильности переходить к suppression.

## 40. Virtual HID milestone criterion

Следующий milestone:

```text
Rust test client
→ NumFlow VHF driver
→ Virtual HID Mouse
→ Windows cursor moves
```

Для этого backend не используется `SendInput`.

После этого соединить два pipeline.

## 41. Финальная architecture

Целевой production path:

```text
Physical NumPad
↓
NumFlow Keyboard Filter
↓
NumFlow Service
↓
NumFlow Core
↓
bindings
↓
velocity / acceleration
↓
Virtual HID Mouse
↓
Windows pointer system
```

Fallback:

```text
Physical NumPad
↓
WH_KEYBOARD_LL
↓
NumFlow Core
↓
SendInput
```

Оба backend используют один и тот же NumFlow Core и одинаковые mappings.

## 42. Не менять UI раньше времени

До рабочего driver PoC не заниматься большим UI refactor.

Допустимо добавить только diagnostics:

```text
Driver
Installed

Service
Connected

Backend
Standard / Driver

Keyboard devices
2

Virtual mouse
Ready

Mode
PASS_THROUGH / ACTIVE
```

## 43. Документация

Создать:

- `docs/windows-driver-architecture.md`;
- `docs/windows-driver-development.md`;
- `docs/windows-driver-recovery.md`;
- `docs/windows-driver-signing.md`;
- `docs/windows-driver-testing.md`.

Документация должна содержать:

- architecture diagram;
- driver stack position;
- protocol;
- install/uninstall;
- Safe Mode recovery;
- signing;
- debugging;
- known limitations.

## 44. Перед написанием кода

Сначала предоставить короткий architecture report:

1. текущий NumFlow input path;
2. текущий mouse output path;
3. где остаётся `WH_KEYBOARD_LL`;
4. где остаётся `SendInput`;
5. какой keyboard filter architecture предлагается;
6. как она работает с USB/Bluetooth;
7. почему выбран именно этот уровень stack;
8. как будет устроен driver ↔ service protocol;
9. как будет работать fail-safe;
10. как будет реализован Virtual HID Mouse;
11. какие WDK/Microsoft samples будут использоваться;
12. потенциальные BSOD/installation risks.

Только после этого начинать Phase 1.

## Главные engineering principles

Kernel code должен быть минимальным.

Driver не содержит business logic.

Failure всегда означает `PASS_THROUGH`, а не keyboard unavailable.

Никакой один crash NumFlow не должен лишить пользователя физической клавиатуры.

Virtual HID Mouse должен заменить `SendInput` только после успешного независимого тестирования.

User-mode backend остаётся fallback.

Не считать задачу завершённой после успешной компиляции.

Главный критерий — реальная стабильность:

- Sleep/Resume;
- USB reconnect;
- Task Manager;
- multiple keyboards;
- service crash;
- 10+ последовательных lifecycle cycles.

Всё это должно работать без BSOD, зависшей клавиатуры, потерянных NumPad events и stuck mouse buttons.
