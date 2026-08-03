use serde::Deserialize;
use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::File;

const LOG_PATH: &str = "wardrobe.log";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }

    const fn filter(self) -> LevelFilter {
        match self {
            Self::Error => LevelFilter::Error,
            Self::Warn => LevelFilter::Warn,
            Self::Info => LevelFilter::Info,
            Self::Debug => LevelFilter::Debug,
            Self::Trace => LevelFilter::Trace,
        }
    }
}

pub fn init() {
    if let Ok(file) = File::create(LOG_PATH) {
        let config = ConfigBuilder::new()
            .set_time_level(LevelFilter::Error)
            .set_time_format_rfc3339()
            .build();
        let _ = WriteLogger::init(LevelFilter::Trace, config, file);
    }
}

/// Changes the process-wide filter while retaining a trace-capable file logger.
pub fn set_level(level: LogLevel) {
    log::set_max_level(level.filter());
}

#[cfg(test)]
mod tests {
    use super::LogLevel;

    #[test]
    fn names_each_configured_log_level() {
        assert_eq!(LogLevel::Error.name(), "error");
        assert_eq!(LogLevel::Warn.name(), "warn");
        assert_eq!(LogLevel::Info.name(), "info");
        assert_eq!(LogLevel::Debug.name(), "debug");
        assert_eq!(LogLevel::Trace.name(), "trace");
    }
}
