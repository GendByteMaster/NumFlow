pub(crate) fn prepare_after_ui() -> Result<(), String> {
    Err(
        "Linux global input backend is not implemented; it requires an evdev/X11/Wayland-specific capture and permission implementation"
            .to_owned(),
    )
}
