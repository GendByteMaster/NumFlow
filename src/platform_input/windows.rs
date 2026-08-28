pub(crate) fn prepare_after_ui() -> Result<(), String> {
    numflow_windows::disable_winit_raw_keyboard_registration().map_err(|error| {
        format!("failed to establish focus-independent global keyboard capture: {error}")
    })
}
