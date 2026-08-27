from pathlib import Path


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise RuntimeError(f"expected exactly one match in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


design = Path("ui/design-system.slint")
replace_once(
    design,
    "    in property <brush> material: Palette.background.with-alpha(0.88);\n"
    "    in property <brush> material-raised: Palette.alternate-background.with-alpha(0.72);\n"
    "    in property <brush> control: Palette.control-background.with-alpha(0.78);",
    "    // Keep the native Mica influence subtle: desktop content must never remain readable\n"
    "    // through the utility surface. Raised controls retain a little more translucency.\n"
    "    in property <brush> material: Palette.background.with-alpha(0.97);\n"
    "    in property <brush> material-raised: Palette.alternate-background.with-alpha(0.90);\n"
    "    in property <brush> hud-material: Palette.background.with-alpha(0.94);\n"
    "    in property <brush> control: Palette.control-background.with-alpha(0.88);",
)

hud_ui = Path("ui/hud.slint")
replace_once(
    hud_ui,
    "        background: NumFlowTheme.material;",
    "        // HUD translucency is clipped to the rounded card instead of the full native window.\n"
    "        background: NumFlowTheme.hud-material;",
)

hud_rs = Path("src/hud.rs")
replace_once(
    hud_rs,
    "    platform::windows::{BackdropType, WindowExtWindows},",
    "    platform::windows::WindowExtWindows,",
)
replace_once(
    hud_rs,
    "    // Winit maintains the shell-facing skip-taskbar state (including Explorer restarts).\n"
    "    winit_window.set_skip_taskbar(true);\n"
    "    // TransientWindow maps to the Windows Background Acrylic system backdrop when available.\n"
    "    // Older Windows versions keep the Slint translucent material fallback.\n"
    "    winit_window.set_system_backdrop(BackdropType::TransientWindow);",
    "    // Winit maintains the shell-facing skip-taskbar state (including Explorer restarts).\n"
    "    winit_window.set_skip_taskbar(true);\n"
    "    // Do not apply Acrylic to the full rectangular HWND. The HUD itself draws a rounded,\n"
    "    // high-contrast translucent material; keeping the native window transparent avoids a\n"
    "    // visible rectangular backdrop around that card while preserving click-through behavior.",
)
