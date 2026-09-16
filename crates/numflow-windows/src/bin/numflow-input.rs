#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    let input_service_requested = std::env::args_os()
        .skip(1)
        .any(|argument| argument == "--input-service");

    if !input_service_requested {
        std::process::exit(2);
    }

    if let Err(error) = numflow_windows::run_input_helper() {
        eprintln!("NumFlow input helper failed: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("numflow-input is only supported on Windows");
    std::process::exit(1);
}
