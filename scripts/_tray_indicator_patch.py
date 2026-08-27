from pathlib import Path

APP_PATH = Path("src/app.rs")
TRAY_PATH = Path("ui/tray.slint")
ICON_DIR = Path("assets/icons/numflow")


def mouse_svg(state: str, button: str | None = None) -> str:
    lines = ['<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">']
    if state == "off":
        lines.extend([
            '<path d="M7 3h18a4 4 0 0 1 4 4v11a11 11 0 0 1-11 11h-4A11 11 0 0 1 3 18V7a4 4 0 0 1 4-4z" fill="none" stroke="#A7A7A7" stroke-width="2"/>',
            '<path d="M6 6l20 20" stroke="#A7A7A7" stroke-width="2" stroke-linecap="round"/>',
        ])
    else:
        lines.extend([
            '<path d="M7 3h18a4 4 0 0 1 4 4v11a11 11 0 0 1-11 11h-4A11 11 0 0 1 3 18V7a4 4 0 0 1 4-4z" fill="#F2F2F2" stroke="#8A8A8A" stroke-width="1.5"/>',
            '<path d="M4 13h24" stroke="#666" stroke-width="1.2"/>',
            '<path d="M12.5 4v9M19.5 4v9" stroke="#666" stroke-width="1.2"/>',
        ])
        rects = {
            "left": (4.8, 4.2, 7.0, 8.0),
            "middle": (13.1, 4.2, 5.8, 8.0),
            "right": (20.2, 4.2, 7.0, 8.0),
        }
        x, y, width, height = rects[button]
        if state == "held":
            lines.append(f'<rect x="{x}" y="{y}" width="{width}" height="{height}" rx="0.8" fill="#111"/>')
        else:
            step = 2.4
            size = 1.2
            row = 0
            yy = y + 0.5
            while yy + size <= y + height:
                col = 0
                xx = x + 0.5
                while xx + size <= x + width:
                    if (row + col) % 2 == 0:
                        lines.append(f'<rect x="{xx:.1f}" y="{yy:.1f}" width="{size}" height="{size}" fill="#111"/>')
                    xx += step
                    col += 1
                yy += step
                row += 1
    lines.append("</svg>")
    return "\n".join(lines) + "\n"


app = APP_PATH.read_text(encoding="utf-8")
old = '''fn sync_tray_from_settings(tray: &AppTray, settings: &UiSettings) {
    tray.set_numflow_enabled(settings.enabled());
    tray.set_start_minimized(settings.start_minimized());
    tray.set_start_with_windows(settings.start_with_windows());
}
'''
new = '''fn sync_tray_from_settings(tray: &AppTray, settings: &UiSettings) {
    tray.set_numflow_enabled(settings.enabled());
    tray.set_active_button(map_mouse_button_to_ui(
        settings.controller.selected_button(),
    ));

    if let Some(held_button) = settings.held_button() {
        tray.set_button_held(true);
        tray.set_held_button(map_mouse_button_to_ui(held_button));
    } else {
        tray.set_button_held(false);
        tray.set_held_button(map_mouse_button_to_ui(
            settings.controller.selected_button(),
        ));
    }

    tray.set_start_minimized(settings.start_minimized());
    tray.set_start_with_windows(settings.start_with_windows());
}
'''
if app.count(old) != 1:
    raise SystemExit("sync_tray_from_settings marker mismatch")
APP_PATH.write_text(app.replace(old, new), encoding="utf-8")

TRAY_PATH.write_text('''import { MouseButtonMode } from "app.slint";

export component AppTray inherits SystemTrayIcon {
    in-out property <bool> numflow-enabled: false;
    in-out property <MouseButtonMode> active-button: MouseButtonMode.left;
    in-out property <bool> button-held: false;
    in-out property <MouseButtonMode> held-button: MouseButtonMode.left;
    in-out property <bool> start-minimized: false;
    in-out property <bool> start-with-windows: false;

    private property <string> active-button-label:
        root.active-button == MouseButtonMode.left ? "Left" :
        root.active-button == MouseButtonMode.right ? "Right" : "Middle";

    private property <string> held-button-label:
        root.held-button == MouseButtonMode.left ? "Left" :
        root.held-button == MouseButtonMode.right ? "Right" : "Middle";

    private property <image> status-icon:
        !root.numflow-enabled
            ? @image-url("../assets/icons/numflow/tray-mouse-off.svg")
            : root.button-held
                ? root.held-button == MouseButtonMode.left
                    ? @image-url("../assets/icons/numflow/tray-mouse-left-held.svg")
                    : root.held-button == MouseButtonMode.right
                        ? @image-url("../assets/icons/numflow/tray-mouse-right-held.svg")
                        : @image-url("../assets/icons/numflow/tray-mouse-middle-held.svg")
                : root.active-button == MouseButtonMode.left
                    ? @image-url("../assets/icons/numflow/tray-mouse-left.svg")
                    : root.active-button == MouseButtonMode.right
                        ? @image-url("../assets/icons/numflow/tray-mouse-right.svg")
                        : @image-url("../assets/icons/numflow/tray-mouse-middle.svg");

    icon: root.status-icon;
    title: "NumFlow";
    tooltip:
        !root.numflow-enabled
            ? "NumFlow · Off · Num Lock On · NumPad numbers"
            : root.button-held
                ? "NumFlow · On · " + root.held-button-label + " held · Num Lock Off"
                : "NumFlow · On · " + root.active-button-label + " selected · Num Lock Off";

    callback open-settings();
    callback enabled-toggled(bool);
    callback start-minimized-toggled(bool);
    callback start-with-windows-toggled(bool);
    callback exit-requested();

    clicked => { root.open-settings(); }

    Menu {
        MenuItem {
            title: "Open NumFlow";
            activated => { root.open-settings(); }
        }

        MenuSeparator {}

        MenuItem {
            title: root.numflow-enabled
                ? "NumFlow On · Num Lock Off · Pointer control"
                : "NumFlow Off · Num Lock On · Numbers";
            checkable: true;
            checked: root.numflow-enabled;
            enabled: false;
        }

        MenuItem {
            title: root.button-held
                ? "Mouse · " + root.held-button-label + " held"
                : "Mouse · " + root.active-button-label + " selected";
            enabled: false;
        }

        MenuItem {
            title: "Running in background";
            enabled: false;
        }

        MenuSeparator {}

        MenuItem {
            title: "Start minimized";
            checkable: true;
            checked: root.start-minimized;
            activated => {
                root.start-minimized = !root.start-minimized;
                root.start-minimized-toggled(root.start-minimized);
            }
        }

        MenuItem {
            title: "Start with Windows";
            checkable: true;
            checked: root.start-with-windows;
            activated => {
                root.start-with-windows = !root.start-with-windows;
                root.start-with-windows-toggled(root.start-with-windows);
            }
        }

        MenuSeparator {}

        MenuItem {
            title: "Exit NumFlow";
            activated => { root.exit-requested(); }
        }
    }
}
''', encoding="utf-8")

ICON_DIR.mkdir(parents=True, exist_ok=True)
(ICON_DIR / "tray-mouse-off.svg").write_text(mouse_svg("off"), encoding="utf-8")
for button in ("left", "middle", "right"):
    (ICON_DIR / f"tray-mouse-{button}.svg").write_text(mouse_svg("selected", button), encoding="utf-8")
    (ICON_DIR / f"tray-mouse-{button}-held.svg").write_text(mouse_svg("held", button), encoding="utf-8")
