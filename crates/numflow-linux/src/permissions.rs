use std::{io, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LinuxInputError {
    #[error("cannot read Linux input device {path}: {source}")]
    InputRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot exclusively grab Linux keyboard {path}: {source}")]
    InputGrab {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot release exclusive Linux keyboard {path}: {source}")]
    InputUngrab {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot open /dev/uinput: {0}")]
    UinputOpen(io::Error),
    #[error("cannot create or write NumFlow virtual device: {0}")]
    VirtualDevice(io::Error),
    #[error("Linux input runtime is unavailable: {0}")]
    Runtime(String),
}
