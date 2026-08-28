pub(crate) fn prepare_after_ui() -> Result<(), String> {
    Err(
        "macOS global input backend is not implemented; it requires an accessibility-authorized CGEventTap implementation"
            .to_owned(),
    )
}
