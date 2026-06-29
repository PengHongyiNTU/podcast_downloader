use std::{fs::OpenOptions, io, path::Path};

use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};

use crate::Result;

pub fn init_file_logger(log_file_path: Option<&Path>) -> Result<()> {
    let Some(path) = log_file_path else {
        return Ok(());
    };
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    WriteLogger::init(LevelFilter::Info, LogConfig::default(), file)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(())
}
