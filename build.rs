fn main() {
    for path in [
        "ui/main.slint",
        "ui/app.slint",
        "ui/design-system.slint",
        "ui/tray.slint",
        "ui/hud.slint",
        "assets/icons/numflow/tray-mouse-left.svg",
        "assets/icons/numflow/tray-mouse-middle.svg",
        "assets/icons/numflow/tray-mouse-right.svg",
        "assets/icons/numflow/tray-mouse-left-held.svg",
        "assets/icons/numflow/tray-mouse-middle-held.svg",
        "assets/icons/numflow/tray-mouse-right-held.svg",
        "assets/icons/numflow/tray-mouse-off.svg",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    slint_build::compile("ui/main.slint").expect("failed to compile Slint UI");
}
