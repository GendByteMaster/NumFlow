from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
DESIGN = ROOT / "ui" / "design-system.slint"
UI = ROOT / "ui" / "app.slint"
HUD_UI = ROOT / "ui" / "hud.slint"
APP_RS = ROOT / "src" / "app.rs"
HUD_RS = ROOT / "src" / "hud.rs"


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{label}: expected exactly one match, found {count}")
    return text.replace(old, new, 1)


DESIGN.write_text(
    r'''import { Palette } from "std-widgets.slint";

export global NumFlowTheme {
    in property <brush> background: Palette.background;
    in property <brush> surface: Palette.alternate-background;
    in property <brush> material: Palette.background.with-alpha(0.88);
    in property <brush> material-raised: Palette.alternate-background.with-alpha(0.72);
    in property <brush> control: Palette.control-background.with-alpha(0.78);
    in property <brush> label: Palette.foreground;
    in property <brush> secondary-label: Palette.alternate-foreground;
    in property <brush> tertiary-label: Palette.alternate-foreground.with-alpha(0.70);
    in property <brush> separator: Palette.border.with-alpha(0.32);
    in property <color> accent: #0a84ff;
    in property <color> accent-secondary: #64d2ff;
    in property <color> accent-foreground: #ffffff;
    in property <brush> accent-soft: #0a84ff20;
    in property <brush> accent-strong: #0a84ff38;
    in property <brush> hover: Palette.foreground.with-alpha(0.055);
    in property <brush> pressed: Palette.foreground.with-alpha(0.10);
    in property <color> focus-ring: #64d2ff;
    in property <color> status-active: #30d158;
    in property <color> destructive: #ff453a;
    in property <brush> destructive-soft: #ff453a1c;
    in property <color> glass-border: #ffffff2a;
    in property <color> glass-highlight: #ffffff32;
    in property <color> shadow-color: #00000058;

    in property <length> radius-small: 7px;
    in property <length> radius-medium: 10px;
    in property <length> radius-large: 13px;
    in property <length> control-height: 36px;
    in property <length> key-height: 48px;
}

export component Separator inherits Rectangle {
    min-height: 1px;
    max-height: 1px;
    background: @linear-gradient(90deg, #ffffff00 0%, NumFlowTheme.separator 12%, NumFlowTheme.separator 88%, #ffffff00 100%);
}

component SegmentedCell inherits Rectangle {
    in property <string> label;
    in property <bool> selected: false;
    in property <bool> enabled: true;
    in property <bool> reduced-motion: false;
    callback clicked();

    background: transparent;
    border-radius: NumFlowTheme.radius-small;
    border-width: keyboard-focus.has-focus ? 2px : 0px;
    border-color: keyboard-focus.has-focus ? NumFlowTheme.focus-ring : transparent;
    opacity: root.enabled ? 1 : 0.48;

    hover-surface := Rectangle {
        width: parent.width;
        height: parent.height;
        border-radius: NumFlowTheme.radius-small;
        background: interaction.pressed ? NumFlowTheme.pressed : NumFlowTheme.hover;
        opacity: interaction.pressed ? 1 : interaction.has-hover && root.enabled ? 1 : 0;
        animate opacity {
            duration: root.reduced-motion ? 0ms : 120ms;
            easing: ease-out;
        }
    }

    HorizontalLayout {
        padding-left: 7px;
        padding-right: 7px;
        spacing: 5px;
        alignment: center;

        Text {
            text: root.selected ? "✓" : "";
            color: NumFlowTheme.accent-secondary;
            font-size: 10px;
            font-weight: 700;
            vertical-alignment: center;
        }

        Text {
            text: root.label;
            color: NumFlowTheme.label;
            font-size: 12px;
            font-weight: root.selected ? 600 : 450;
            vertical-alignment: center;
        }
    }

    keyboard-focus := FocusScope {
        width: parent.width;
        height: parent.height;
        enabled: root.enabled;
        focus-on-click: true;
        focus-on-tab-navigation: true;
        accessible-role: radio-button;
        accessible-label: root.label;
        accessible-checkable: true;
        accessible-checked: root.selected;
        accessible-action-default => { root.clicked(); }

        KeyBinding { keys: @keys(Return); activated => { root.clicked(); } }
        KeyBinding { keys: @keys(Space); activated => { root.clicked(); } }

        interaction := TouchArea {
            enabled: root.enabled;
            clicked => {
                keyboard-focus.focus();
                root.clicked();
            }
        }
    }
}

export component SegmentedControl inherits Rectangle {
    in property <string> first-label;
    in property <string> second-label;
    in property <string> third-label;
    in-out property <int> selected-index: 0;
    in property <bool> enabled: true;
    in property <bool> reduced-motion: false;
    callback selected(int);

    private property <length> segment-width: (root.width - 6px) / 3;

    min-height: NumFlowTheme.control-height;
    max-height: NumFlowTheme.control-height;
    background: NumFlowTheme.material-raised;
    border-width: 1px;
    border-color: NumFlowTheme.glass-border;
    border-radius: NumFlowTheme.radius-medium;
    opacity: root.enabled ? 1 : 0.58;
    drop-shadow-blur: 7px;
    drop-shadow-color: #00000022;
    drop-shadow-offset-y: 2px;

    Rectangle {
        x: 1px;
        y: 1px;
        width: parent.width - 2px;
        height: 1px;
        background: @linear-gradient(90deg, #ffffff00 0%, #ffffff24 50%, #ffffff00 100%);
    }

    selected-indicator := Rectangle {
        x: root.selected-index == 0
            ? 3px
            : root.selected-index == 1
                ? 3px + root.segment-width
                : 3px + root.segment-width * 2;
        y: 3px;
        width: root.segment-width;
        height: root.height - 6px;
        background: @linear-gradient(135deg, #0a84ff36 0%, #64d2ff18 100%);
        border-width: 1px;
        border-color: #64d2ff62;
        border-radius: NumFlowTheme.radius-small;
        drop-shadow-blur: 8px;
        drop-shadow-color: #0a84ff24;
        drop-shadow-offset-y: 1px;

        Rectangle {
            x: 1px;
            y: 1px;
            width: parent.width - 2px;
            height: 1px;
            background: @linear-gradient(90deg, #ffffff08 0%, #ffffff42 50%, #ffffff08 100%);
        }

        animate x {
            duration: root.reduced-motion ? 0ms : 210ms;
            easing: ease-out;
        }
    }

    SegmentedCell {
        x: 3px;
        y: 3px;
        width: root.segment-width;
        height: root.height - 6px;
        label: root.first-label;
        selected: root.selected-index == 0;
        enabled: root.enabled;
        reduced-motion: root.reduced-motion;
        clicked => { root.selected-index = 0; root.selected(0); }
    }

    SegmentedCell {
        x: 3px + root.segment-width;
        y: 3px;
        width: root.segment-width;
        height: root.height - 6px;
        label: root.second-label;
        selected: root.selected-index == 1;
        enabled: root.enabled;
        reduced-motion: root.reduced-motion;
        clicked => { root.selected-index = 1; root.selected(1); }
    }

    SegmentedCell {
        x: 3px + root.segment-width * 2;
        y: 3px;
        width: root.segment-width;
        height: root.height - 6px;
        label: root.third-label;
        selected: root.selected-index == 2;
        enabled: root.enabled;
        reduced-motion: root.reduced-motion;
        clicked => { root.selected-index = 2; root.selected(2); }
    }
}

export component GlassToggle inherits Rectangle {
    in-out property <bool> checked: false;
    in property <bool> enabled: true;
    in property <bool> reduced-motion: false;
    in property <string> a11y-label: "Toggle";
    callback toggled(bool);

    min-width: 44px;
    max-width: 44px;
    min-height: 26px;
    max-height: 26px;
    border-radius: 13px;
    border-width: keyboard-focus.has-focus ? 2px : 1px;
    border-color: keyboard-focus.has-focus
        ? NumFlowTheme.focus-ring
        : root.checked
            ? #64d2ff70
            : NumFlowTheme.glass-border;
    background: root.checked
        ? @linear-gradient(100deg, #0a84ff 0%, #46a9ff 58%, #64d2ff 100%)
        : NumFlowTheme.material-raised;
    opacity: root.enabled ? 1 : 0.48;
    drop-shadow-blur: root.checked ? 8px : 4px;
    drop-shadow-color: root.checked ? #0a84ff32 : #00000020;
    drop-shadow-offset-y: 1px;

    Rectangle {
        width: parent.width;
        height: parent.height;
        border-radius: parent.border-radius;
        background: @linear-gradient(180deg, #ffffff20 0%, #ffffff00 48%);
        opacity: interaction.has-hover && root.enabled ? 1 : 0.55;
        animate opacity {
            duration: root.reduced-motion ? 0ms : 120ms;
            easing: ease-out;
        }
    }

    knob := Rectangle {
        x: root.checked ? root.width - self.width - 4px : 4px;
        y: 4px;
        width: 18px;
        height: 18px;
        border-radius: 9px;
        background: #ffffff;
        drop-shadow-blur: 5px;
        drop-shadow-color: #00000055;
        drop-shadow-offset-y: 1px;

        animate x {
            duration: root.reduced-motion ? 0ms : 190ms;
            easing: ease-out;
        }
    }

    keyboard-focus := FocusScope {
        width: parent.width;
        height: parent.height;
        enabled: root.enabled;
        focus-on-click: true;
        focus-on-tab-navigation: true;
        accessible-role: check-box;
        accessible-label: root.a11y-label;
        accessible-checkable: true;
        accessible-checked: root.checked;
        accessible-action-default => {
            root.checked = !root.checked;
            root.toggled(root.checked);
        }

        KeyBinding {
            keys: @keys(Return);
            activated => {
                root.checked = !root.checked;
                root.toggled(root.checked);
            }
        }
        KeyBinding {
            keys: @keys(Space);
            activated => {
                root.checked = !root.checked;
                root.toggled(root.checked);
            }
        }

        interaction := TouchArea {
            enabled: root.enabled;
            clicked => {
                keyboard-focus.focus();
                root.checked = !root.checked;
                root.toggled(root.checked);
            }
        }
    }
}

export component UtilityButton inherits Rectangle {
    in property <string> text;
    in property <bool> destructive: false;
    in property <bool> enabled: true;
    in property <bool> reduced-motion: false;
    callback clicked();

    min-height: 32px;
    max-height: 32px;
    min-width: 32px;
    border-radius: NumFlowTheme.radius-small;
    border-width: keyboard-focus.has-focus ? 2px : 1px;
    border-color: keyboard-focus.has-focus
        ? NumFlowTheme.focus-ring
        : root.destructive
            ? #ff453a50
            : NumFlowTheme.glass-border;
    background: NumFlowTheme.material-raised;
    opacity: root.enabled ? 1 : 0.48;

    hover-surface := Rectangle {
        width: parent.width;
        height: parent.height;
        border-radius: parent.border-radius;
        background: interaction.pressed
            ? root.destructive ? NumFlowTheme.destructive-soft : NumFlowTheme.pressed
            : root.destructive ? NumFlowTheme.destructive-soft : NumFlowTheme.hover;
        opacity: interaction.pressed ? 1 : interaction.has-hover && root.enabled ? 1 : 0;
        animate opacity {
            duration: root.reduced-motion ? 0ms : 120ms;
            easing: ease-out;
        }
    }

    Rectangle {
        x: 1px;
        y: 1px;
        width: parent.width - 2px;
        height: 1px;
        background: @linear-gradient(90deg, #ffffff00 0%, #ffffff24 50%, #ffffff00 100%);
    }

    Text {
        text: root.text;
        color: root.destructive ? NumFlowTheme.destructive : NumFlowTheme.label;
        font-size: 12px;
        font-weight: 500;
        horizontal-alignment: center;
        vertical-alignment: center;
    }

    keyboard-focus := FocusScope {
        width: parent.width;
        height: parent.height;
        enabled: root.enabled;
        focus-on-click: true;
        focus-on-tab-navigation: true;
        accessible-role: button;
        accessible-label: root.text;
        accessible-action-default => { root.clicked(); }

        KeyBinding { keys: @keys(Return); activated => { root.clicked(); } }
        KeyBinding { keys: @keys(Space); activated => { root.clicked(); } }

        interaction := TouchArea {
            enabled: root.enabled;
            clicked => {
                keyboard-focus.focus();
                root.clicked();
            }
        }
    }
}

export component DisclosureRow inherits Rectangle {
    in property <string> label;
    in property <bool> expanded: false;
    in property <bool> reduced-motion: false;
    callback toggled();

    min-height: 34px;
    max-height: 34px;
    border-radius: NumFlowTheme.radius-small;
    background: transparent;
    border-width: keyboard-focus.has-focus ? 2px : 0px;
    border-color: keyboard-focus.has-focus ? NumFlowTheme.focus-ring : transparent;

    hover-surface := Rectangle {
        width: parent.width;
        height: parent.height;
        border-radius: parent.border-radius;
        background: interaction.pressed ? NumFlowTheme.pressed : NumFlowTheme.hover;
        opacity: interaction.pressed ? 1 : interaction.has-hover ? 1 : 0;
        animate opacity {
            duration: root.reduced-motion ? 0ms : 120ms;
            easing: ease-out;
        }
    }

    HorizontalLayout {
        padding-left: 2px;
        padding-right: 4px;
        spacing: 8px;
        cross-axis-alignment: center;

        Text {
            text: root.label;
            color: NumFlowTheme.label;
            font-size: 12px;
            font-weight: 500;
        }

        Rectangle { horizontal-stretch: 1; background: transparent; }

        Text {
            text: root.expanded ? "⌄" : "›";
            color: root.expanded ? NumFlowTheme.accent-secondary : NumFlowTheme.secondary-label;
            font-size: 17px;
            font-weight: 500;
            opacity: interaction.has-hover || root.expanded ? 1 : 0.78;
            animate opacity {
                duration: root.reduced-motion ? 0ms : 120ms;
            }
        }
    }

    keyboard-focus := FocusScope {
        width: parent.width;
        height: parent.height;
        focus-on-click: true;
        focus-on-tab-navigation: true;
        accessible-role: button;
        accessible-label: root.label;
        accessible-action-default => { root.toggled(); }

        KeyBinding { keys: @keys(Return); activated => { root.toggled(); } }
        KeyBinding { keys: @keys(Space); activated => { root.toggled(); } }

        interaction := TouchArea {
            clicked => {
                keyboard-focus.focus();
                root.toggled();
            }
        }
    }
}

export component MappingRow inherits Rectangle {
    in property <string> key-label;
    in property <string> action-label;

    min-height: 27px;
    max-height: 27px;
    background: transparent;

    HorizontalLayout {
        spacing: 10px;
        cross-axis-alignment: center;

        Rectangle {
            min-width: 38px;
            max-width: 38px;
            min-height: 22px;
            max-height: 22px;
            border-radius: 6px;
            background: NumFlowTheme.material-raised;
            border-width: 1px;
            border-color: NumFlowTheme.glass-border;

            Rectangle {
                x: 1px;
                y: 1px;
                width: parent.width - 2px;
                height: 1px;
                background: #ffffff20;
            }

            Text {
                text: root.key-label;
                color: NumFlowTheme.label;
                font-size: 11px;
                font-weight: 600;
                horizontal-alignment: center;
                vertical-alignment: center;
            }
        }

        Text {
            text: root.action-label;
            color: NumFlowTheme.secondary-label;
            font-size: 11px;
            overflow: elide;
        }
    }
}

export component MenuAction inherits Rectangle {
    in property <string> text;
    in property <bool> destructive: false;
    in property <bool> reduced-motion: false;
    callback clicked();

    min-height: 34px;
    max-height: 34px;
    border-radius: NumFlowTheme.radius-small;
    background: transparent;
    border-width: keyboard-focus.has-focus ? 2px : 0px;
    border-color: keyboard-focus.has-focus ? NumFlowTheme.focus-ring : transparent;

    hover-surface := Rectangle {
        width: parent.width;
        height: parent.height;
        border-radius: parent.border-radius;
        background: interaction.pressed
            ? root.destructive ? NumFlowTheme.destructive-soft : NumFlowTheme.pressed
            : root.destructive ? NumFlowTheme.destructive-soft : NumFlowTheme.hover;
        opacity: interaction.pressed ? 1 : interaction.has-hover ? 1 : 0;
        animate opacity {
            duration: root.reduced-motion ? 0ms : 120ms;
            easing: ease-out;
        }
    }

    Text {
        x: 10px;
        width: parent.width - 20px;
        text: root.text;
        color: root.destructive ? NumFlowTheme.destructive : NumFlowTheme.label;
        font-size: 12px;
        font-weight: root.destructive ? 550 : 450;
        vertical-alignment: center;
    }

    keyboard-focus := FocusScope {
        width: parent.width;
        height: parent.height;
        focus-on-click: true;
        focus-on-tab-navigation: true;
        accessible-role: button;
        accessible-label: root.text;
        accessible-action-default => { root.clicked(); }

        KeyBinding { keys: @keys(Return); activated => { root.clicked(); } }
        KeyBinding { keys: @keys(Space); activated => { root.clicked(); } }

        interaction := TouchArea {
            clicked => {
                keyboard-focus.focus();
                root.clicked();
            }
        }
    }
}
''',
    encoding="utf-8",
)

HUD_UI.write_text(
    r'''import { NumFlowTheme } from "design-system.slint";

export enum HudIconKind {
    power-on,
    power-off,
    left,
    right,
    middle,
    precision,
    dragging,
    info,
}

export component HudWindow inherits Window {
    title: "NumFlow HUD";
    width: 288px;
    height: 84px;
    background: transparent;
    no-frame: true;
    always-on-top: true;

    in-out property <HudIconKind> icon-kind: HudIconKind.info;
    in-out property <string> headline: "NumFlow";
    in-out property <string> detail: "Ready";
    in-out property <bool> persistent: false;
    in-out property <bool> revealed: false;
    in-out property <bool> reduced-motion: false;

    private property <image> icon-source:
        root.icon-kind == HudIconKind.power-on
            ? @image-url("../assets/icons/numflow/power-on.svg")
            : root.icon-kind == HudIconKind.power-off
                ? @image-url("../assets/icons/numflow/power-off.svg")
                : root.icon-kind == HudIconKind.left
                    ? @image-url("../assets/icons/numflow/mouse-left.svg")
                    : root.icon-kind == HudIconKind.right
                        ? @image-url("../assets/icons/numflow/mouse-right.svg")
                        : root.icon-kind == HudIconKind.middle
                            ? @image-url("../assets/icons/numflow/mouse-middle.svg")
                            : root.icon-kind == HudIconKind.precision
                                ? @image-url("../assets/icons/numflow/precision-mode.svg")
                                : root.icon-kind == HudIconKind.dragging
                                    ? @image-url("../assets/icons/numflow/drag-lock.svg")
                                    : @image-url("../assets/numflow-icon.svg");

    glass := Rectangle {
        x: 8px;
        y: root.revealed ? 6px : 10px;
        width: parent.width - 16px;
        height: parent.height - 12px;
        opacity: root.revealed ? 1 : 0;
        background: NumFlowTheme.material;
        border-width: root.persistent ? 1px : 1px;
        border-color: root.persistent ? #64d2ff82 : NumFlowTheme.glass-border;
        border-radius: 17px;
        drop-shadow-blur: 18px;
        drop-shadow-color: NumFlowTheme.shadow-color;
        drop-shadow-offset-y: 5px;

        animate y, opacity {
            duration: root.reduced-motion ? 0ms : 180ms;
            easing: ease-out;
        }

        Rectangle {
            width: parent.width;
            height: parent.height;
            border-radius: parent.border-radius;
            background: @linear-gradient(135deg, #ffffff14 0%, #ffffff05 44%, #0a84ff12 100%);
        }

        Rectangle {
            x: 1px;
            y: 1px;
            width: parent.width - 2px;
            height: 1px;
            background: @linear-gradient(90deg, #ffffff08 0%, #ffffff42 48%, #64d2ff20 75%, #ffffff00 100%);
        }

        Rectangle {
            x: 0px;
            y: 19px;
            width: 2px;
            height: parent.height - 38px;
            border-radius: 1px;
            background: NumFlowTheme.accent-secondary;
            opacity: root.persistent ? 0.95 : 0;
            animate opacity {
                duration: root.reduced-motion ? 0ms : 150ms;
            }
        }

        HorizontalLayout {
            padding-left: 12px;
            padding-right: 12px;
            padding-top: 10px;
            padding-bottom: 10px;
            spacing: 11px;
            cross-axis-alignment: center;

            Rectangle {
                min-width: 42px;
                max-width: 42px;
                min-height: 42px;
                max-height: 42px;
                border-radius: 12px;
                background: @linear-gradient(145deg, #0a84ff30 0%, #64d2ff12 100%);
                border-width: 1px;
                border-color: #64d2ff42;
                drop-shadow-blur: 8px;
                drop-shadow-color: #0a84ff20;

                Rectangle {
                    x: 1px;
                    y: 1px;
                    width: parent.width - 2px;
                    height: 1px;
                    background: #ffffff32;
                }

                Image {
                    x: 7px;
                    y: 7px;
                    width: 28px;
                    height: 28px;
                    source: root.icon-source;
                    image-fit: contain;
                    accessible-role: none;
                }
            }

            VerticalLayout {
                spacing: 2px;
                horizontal-stretch: 1;

                Text {
                    text: root.headline;
                    color: NumFlowTheme.label;
                    font-size: 13px;
                    font-weight: 600;
                    overflow: elide;
                }

                Text {
                    text: root.detail;
                    color: NumFlowTheme.secondary-label;
                    font-size: 10px;
                    overflow: elide;
                }
            }

            Rectangle {
                min-width: 8px;
                max-width: 8px;
                min-height: 8px;
                max-height: 8px;
                border-radius: 4px;
                background: NumFlowTheme.accent-secondary;
                opacity: root.persistent ? 1 : 0;
                drop-shadow-blur: 6px;
                drop-shadow-color: #64d2ff70;
                animate opacity {
                    duration: root.reduced-motion ? 0ms : 150ms;
                }
            }
        }
    }
}
''',
    encoding="utf-8",
)

ui = UI.read_text(encoding="utf-8")
ui = replace_once(
    ui,
    'import { Button, ComboBox, Slider, Switch } from "std-widgets.slint";',
    'import { Button, ComboBox, Slider } from "std-widgets.slint";',
    "remove std Switch import",
)
ui = replace_once(
    ui,
    "    DisclosureRow, MappingRow, MenuAction, NumFlowTheme, SegmentedControl,\n    Separator, UtilityButton",
    "    DisclosureRow, GlassToggle, MappingRow, MenuAction, NumFlowTheme, SegmentedControl,\n    Separator, UtilityButton",
    "import GlassToggle",
)
ui = replace_once(
    ui,
    "    background: NumFlowTheme.background;",
    "    background: transparent;",
    "transparent main window for backdrop",
)
ui = replace_once(
    ui,
    '''                    Text {
                        text: root.numflow-enabled ? "●" : "○";
                        color: root.numflow-enabled ? NumFlowTheme.accent : NumFlowTheme.secondary-label;
                        font-size: 12px;
                    }
''',
    '''                    Rectangle {
                        min-width: 14px;
                        max-width: 14px;
                        min-height: 14px;
                        max-height: 14px;
                        background: transparent;

                        Rectangle {
                            x: 0px;
                            y: 0px;
                            width: 14px;
                            height: 14px;
                            border-radius: 7px;
                            background: #30d15820;
                            opacity: root.numflow-enabled ? 1 : 0;
                            animate opacity {
                                duration: root.reduced-motion ? 0ms : 180ms;
                                easing: ease-out;
                            }
                        }

                        Rectangle {
                            x: 4px;
                            y: 4px;
                            width: 6px;
                            height: 6px;
                            border-radius: 3px;
                            background: root.numflow-enabled ? NumFlowTheme.status-active : transparent;
                            border-width: root.numflow-enabled ? 0px : 1px;
                            border-color: NumFlowTheme.secondary-label;
                            drop-shadow-blur: root.numflow-enabled ? 6px : 0px;
                            drop-shadow-color: #30d15872;
                            animate background {
                                duration: root.reduced-motion ? 0ms : 180ms;
                                easing: ease-out;
                            }
                        }
                    }
''',
    "semantic active indicator",
)
ui = replace_once(
    ui,
    '''                    Switch {
                        accessible-label: "NumFlow";
                        text: root.numflow-enabled ? "On" : "Off";
                        checked <=> root.numflow-enabled;
                        toggled => { root.enabled-toggled(self.checked); }
                    }
''',
    '''                    GlassToggle {
                        a11y-label: "NumFlow";
                        checked <=> root.numflow-enabled;
                        reduced-motion: root.reduced-motion;
                        toggled(checked) => { root.enabled-toggled(checked); }
                    }

                    Text {
                        text: root.numflow-enabled ? "On" : "Off";
                        color: NumFlowTheme.secondary-label;
                        font-size: 11px;
                        min-width: 22px;
                        max-width: 22px;
                    }
''',
    "main glass toggle",
)
ui = replace_once(
    ui,
    '''                    Switch {
                        enabled: root.advanced-open;
                        accessible-label: "Precision mode";
                        text: root.precision-enabled ? "On" : "Off";
                        checked <=> root.precision-enabled;
                        toggled => {
                            root.precision-toggled(self.checked);
                            root.mark-saved();
                        }
                    }
''',
    '''                    Text {
                        text: root.precision-enabled ? "On" : "Off";
                        color: NumFlowTheme.secondary-label;
                        font-size: 10px;
                    }

                    GlassToggle {
                        enabled: root.advanced-open;
                        a11y-label: "Precision mode";
                        checked <=> root.precision-enabled;
                        reduced-motion: root.reduced-motion;
                        toggled(checked) => {
                            root.precision-toggled(checked);
                            root.mark-saved();
                        }
                    }
''',
    "precision glass toggle",
)
ui = replace_once(
    ui,
    '''                    Switch {
                        accessible-label: "HUD";
                        text: "";
                        checked <=> root.hud-enabled;
                        toggled => {
                            root.hud-toggled(self.checked);
                            root.mark-saved();
                        }
                    }
''',
    '''                    GlassToggle {
                        a11y-label: "HUD";
                        checked <=> root.hud-enabled;
                        reduced-motion: root.reduced-motion;
                        toggled(checked) => {
                            root.hud-toggled(checked);
                            root.mark-saved();
                        }
                    }
''',
    "HUD glass toggle",
)
ui = replace_once(
    ui,
    '''            background: @linear-gradient(90deg, #ffffff00 0%, #ffffff2b 50%, #ffffff00 100%);
        }

        VerticalLayout {
''',
    '''            background: @linear-gradient(90deg, #0a84ff00 0%, #64d2ff24 24%, #ffffff2c 50%, #0a84ff18 76%, #0a84ff00 100%);
        }

        ambient-top := Rectangle {
            x: -88px;
            y: -148px;
            width: 430px;
            height: 300px;
            background: @radial-gradient(circle, #64d2ff14 0%, #0a84ff09 38%, #0a84ff00 72%);
            opacity: root.numflow-enabled ? 1 : 0.46;
            animate opacity {
                duration: root.reduced-motion ? 0ms : 220ms;
                easing: ease-out;
            }
        }

        Rectangle {
            x: parent.width - 250px;
            y: parent.height - 210px;
            width: 320px;
            height: 250px;
            background: @radial-gradient(circle, #0a84ff0c 0%, #0a84ff00 70%);
        }

        VerticalLayout {
''',
    "ambient material gradients",
)

# Give all custom secondary controls the same reduced-motion behavior without touching std widgets.
ui = re.sub(
    r'(?m)^(\s*)(UtilityButton|MenuAction) \{\n(?!\s*reduced-motion:)',
    lambda match: f'{match.group(1)}{match.group(2)} {{\n{match.group(1)}    reduced-motion: root.reduced-motion;\n',
    ui,
)
UI.write_text(ui, encoding="utf-8")

app_rs = APP_RS.read_text(encoding="utf-8")
insert_after_startup = '''#[cfg(not(windows))]
fn set_windows_startup(_enabled: bool) -> bool {
    true
}
'''
main_material = insert_after_startup + r'''

#[cfg(windows)]
fn configure_main_window_material(window: &AppWindow) {
    use slint::winit_030::{
        WinitWindowAccessor,
        winit::platform::windows::{BackdropType, WindowExtWindows},
    };

    let weak_window = window.as_weak();
    slint::Timer::single_shot(std::time::Duration::ZERO, move || {
        let Some(window) = weak_window.upgrade() else {
            return;
        };

        let configured = window.window().with_winit_window(|winit_window| {
            // Mica is a native Windows 11 system backdrop. Unsupported systems simply keep the
            // translucent Slint material fallback drawn by the UI.
            winit_window.set_system_backdrop(BackdropType::MainWindow);
        });

        if configured.is_none() {
            tracing::warn!(
                "NumFlow glass material requires the Slint winit backend; using translucent fallback"
            );
        }
    });
}

#[cfg(not(windows))]
fn configure_main_window_material(_window: &AppWindow) {}
'''
app_rs = replace_once(
    app_rs,
    insert_after_startup,
    main_material,
    "main Mica helper",
)
app_rs = replace_once(
    app_rs,
    '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;

    #[cfg(windows)]
    match numflow_windows::client_area_animations_enabled() {
        Ok(enabled) => window.set_reduced_motion(!enabled),
        Err(error) => tracing::warn!(
            %error,
            "failed to read Windows client-area animation preference; using standard UI motion"
        ),
    }
''',
    '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    configure_main_window_material(&window);

    #[cfg(windows)]
    let reduced_motion = match numflow_windows::client_area_animations_enabled() {
        Ok(enabled) => !enabled,
        Err(error) => {
            tracing::warn!(
                %error,
                "failed to read Windows client-area animation preference; using standard UI motion"
            );
            false
        }
    };
    #[cfg(not(windows))]
    let reduced_motion = false;
    window.set_reduced_motion(reduced_motion);
''',
    "shared reduced-motion state",
)
app_rs = replace_once(
    app_rs,
    '''    hud.borrow_mut()
        .set_enabled(settings.borrow().hud_enabled());
''',
    '''    hud.borrow_mut()
        .set_enabled(settings.borrow().hud_enabled());
    hud.borrow_mut().set_reduced_motion(reduced_motion);
''',
    "HUD reduced-motion wiring",
)
APP_RS.write_text(app_rs, encoding="utf-8")

hud_rs = HUD_RS.read_text(encoding="utf-8")
hud_rs = replace_once(
    hud_rs,
    '''use slint::winit_030::winit::{
    platform::windows::WindowExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};
''',
    '''use slint::winit_030::winit::{
    platform::windows::{BackdropType, WindowExtWindows},
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
};
''',
    "HUD BackdropType import",
)
hud_rs = replace_once(
    hud_rs,
    '''    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.hide_timer.stop();
            self.persistent.set(false);
            self.hide_window();
        }
    }
''',
    '''    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.hide_timer.stop();
            self.persistent.set(false);
            self.hide_window();
        }
    }

    pub fn set_reduced_motion(&mut self, reduced_motion: bool) {
        self.window.set_reduced_motion(reduced_motion);
    }
''',
    "HUD reduced-motion setter",
)
hud_rs = replace_once(
    hud_rs,
    '''    fn present(&mut self, presentation: &HudPresentation) {
        self.window.set_headline(presentation.headline.into());
''',
    '''    fn present(&mut self, presentation: &HudPresentation) {
        self.window.set_revealed(false);
        self.window.set_headline(presentation.headline.into());
''',
    "HUD reveal reset",
)
hud_rs = replace_once(
    hud_rs,
    '''    fn hide_window(&self) {
        if let Err(error) = self.window.hide() {
''',
    '''    fn hide_window(&self) {
        self.window.set_revealed(false);
        if let Err(error) = self.window.hide() {
''',
    "HUD hide visual state",
)
hud_rs = replace_once(
    hud_rs,
    '''            if configured.is_none() {
                tracing::warn!(
                    "NumFlow HUD requires the Slint winit backend for overlay window behavior"
                );
            }
''',
    '''            if configured.is_none() {
                tracing::warn!(
                    "NumFlow HUD requires the Slint winit backend for overlay window behavior"
                );
            }
            window.set_revealed(true);
''',
    "HUD reveal after native configuration",
)
hud_rs = replace_once(
    hud_rs,
    '''    // Winit maintains the shell-facing skip-taskbar state (including Explorer restarts).
    winit_window.set_skip_taskbar(true);

    let handle = match winit_window.window_handle() {
''',
    '''    // Winit maintains the shell-facing skip-taskbar state (including Explorer restarts).
    winit_window.set_skip_taskbar(true);
    // TransientWindow maps to the Windows Background Acrylic system backdrop when available.
    // Older Windows versions keep the Slint translucent material fallback.
    winit_window.set_system_backdrop(BackdropType::TransientWindow);

    let handle = match winit_window.window_handle() {
''',
    "HUD Acrylic backdrop",
)
HUD_RS.write_text(hud_rs, encoding="utf-8")

# Guardrails: decorative polish must remain finite and the main NumPad UI must stay removed.
all_slint = DESIGN.read_text(encoding="utf-8") + UI.read_text(encoding="utf-8") + HUD_UI.read_text(encoding="utf-8")
if "animation-tick()" in all_slint or "iteration-count" in all_slint:
    raise RuntimeError("looping decorative animation is not allowed")
if 'text: "NumPad"' in UI.read_text(encoding="utf-8"):
    raise RuntimeError("NumPad section must not return to the main window")
