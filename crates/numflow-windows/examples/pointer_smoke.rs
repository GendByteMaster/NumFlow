#[cfg(windows)]
use std::{env, process::ExitCode, thread, time::Duration};

#[cfg(windows)]
use numflow_core::{MouseButton, PointerBackend};
#[cfg(windows)]
use numflow_windows::WindowsPointer;

#[cfg(not(windows))]
fn main() {
    eprintln!("pointer_smoke is available only on Windows");
}

#[cfg(windows)]
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pointer smoke failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return Ok(());
    };

    let mut pointer = WindowsPointer::default();

    match command.as_str() {
        "move" => {
            let dx = parse_i32(args.next(), "dx")?;
            let dy = parse_i32(args.next(), "dy")?;
            ensure_no_extra_args(args)?;
            pointer.move_relative(dx, dy).map_err(|error| error.to_string())
        }
        "click" => {
            let button = parse_button(args.next().as_deref())?;
            ensure_no_extra_args(args)?;
            pointer.click(button).map_err(|error| error.to_string())
        }
        "double-click" => {
            let button = parse_button(args.next().as_deref())?;
            ensure_no_extra_args(args)?;
            pointer
                .double_click(button)
                .map_err(|error| error.to_string())
        }
        "hold" => {
            let button = parse_button(args.next().as_deref())?;
            let milliseconds = parse_u64(args.next(), "milliseconds")?;
            ensure_no_extra_args(args)?;

            pointer
                .button_down(button)
                .map_err(|error| error.to_string())?;
            thread::sleep(Duration::from_millis(milliseconds));
            pointer.button_up(button).map_err(|error| error.to_string())
        }
        _ => Err(format!(
            "unknown command {command:?}; expected move, click, double-click, or hold"
        )),
    }
}

#[cfg(windows)]
fn parse_button(value: Option<&str>) -> Result<MouseButton, String> {
    match value {
        Some("left") => Ok(MouseButton::Left),
        Some("right") => Ok(MouseButton::Right),
        Some("middle") => Ok(MouseButton::Middle),
        Some(other) => Err(format!(
            "unknown mouse button {other:?}; expected left, right, or middle"
        )),
        None => Err("missing mouse button; expected left, right, or middle".to_owned()),
    }
}

#[cfg(windows)]
fn parse_i32(value: Option<String>, name: &str) -> Result<i32, String> {
    value
        .ok_or_else(|| format!("missing {name}"))?
        .parse::<i32>()
        .map_err(|error| format!("invalid {name}: {error}"))
}

#[cfg(windows)]
fn parse_u64(value: Option<String>, name: &str) -> Result<u64, String> {
    value
        .ok_or_else(|| format!("missing {name}"))?
        .parse::<u64>()
        .map_err(|error| format!("invalid {name}: {error}"))
}

#[cfg(windows)]
fn ensure_no_extra_args(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    match args.next() {
        Some(extra) => Err(format!("unexpected extra argument {extra:?}")),
        None => Ok(()),
    }
}

#[cfg(windows)]
fn print_usage() {
    println!(
        "NumFlow pointer smoke commands:\n\
         cargo run -p numflow-windows --example pointer_smoke -- move <dx> <dy>\n\
         cargo run -p numflow-windows --example pointer_smoke -- click <left|right|middle>\n\
         cargo run -p numflow-windows --example pointer_smoke -- double-click <left|right|middle>\n\
         cargo run -p numflow-windows --example pointer_smoke -- hold <left|right|middle> <milliseconds>"
    );
}
