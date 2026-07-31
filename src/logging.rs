use simplelog::{ConfigBuilder, LevelFilter, WriteLogger};
use std::fs::File;

const LOG_PATH: &str = "wardrobe.log";

pub fn init() {
    if let Ok(file) = File::create(LOG_PATH) {
        let config = ConfigBuilder::new()
            .set_time_level(LevelFilter::Error)
            .set_time_format_rfc3339()
            .build();
        let _ = WriteLogger::init(LevelFilter::Debug, config, file);
        log::info!("Wardrobe started");
    }
}
