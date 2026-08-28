#![cfg_attr(not(windows), allow(unused))]

#[cfg(windows)]
fn main() {
    let authorized_launch = std::env::args_os()
        .skip(1)
        .all(|argument| argument == "--secure-runtime")
        && std::env::args_os()
            .skip(1)
            .any(|argument| argument == "--secure-runtime");
    if !authorized_launch {
        eprintln!("NumFlow secure runtime requires the Windows-managed --secure-runtime profile");
        std::process::exit(2);
    }

    let settings = match numflow_windows::SecureSettings::load_for_current_desktop() {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("NumFlow secure runtime rejected copied accessibility settings: {error}");
            std::process::exit(3);
        }
    };

    if let Err(error) = numflow_windows::run_secure_runtime(&settings) {
        eprintln!("NumFlow secure runtime stopped with an error: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("numflow-secure is available only on Windows");
}
