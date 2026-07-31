use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant, SystemTime};

pub const CONFIG_PATH: &str = "custom_skin_loader.json";
const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SkinDefinition {
    pub txd_path: String,
    pub dff_path: String,
    pub donor_model_id: i32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SkinConfig {
    #[serde(default)]
    pub skins: HashMap<String, SkinDefinition>,
    #[serde(default)]
    pub players: HashMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileRevision {
    Present { modified: SystemTime, length: u64 },
    Missing,
    Unreadable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkinSourceRevision {
    pub definition: SkinDefinition,
    pub txd: FileRevision,
    pub dff: FileRevision,
}

pub struct ConfigWatcher {
    last_check: Option<Instant>,
    observed_revision: Option<FileRevision>,
}

impl ConfigWatcher {
    pub fn new() -> Self {
        Self {
            last_check: None,
            observed_revision: None,
        }
    }

    /// Returns a valid replacement configuration only when the file changed.
    /// Invalid edits deliberately leave the active configuration untouched.
    pub fn poll_change(&mut self) -> Option<SkinConfig> {
        let now = Instant::now();
        if self
            .last_check
            .is_some_and(|last_check| now.duration_since(last_check) < CONFIG_RELOAD_INTERVAL)
        {
            return None;
        }
        self.last_check = Some(now);

        let revision = file_revision(CONFIG_PATH);
        if self.observed_revision.as_ref() == Some(&revision) {
            return None;
        }
        self.observed_revision = Some(revision.clone());

        match revision {
            FileRevision::Present { .. } => {}
            FileRevision::Missing => {
                log::error!("{CONFIG_PATH} was removed; keeping the active configuration");
                return None;
            }
            FileRevision::Unreadable(error) => {
                log::error!(
                    "could not inspect changed {CONFIG_PATH}: {error}; keeping the active configuration"
                );
                return None;
            }
        }

        match read() {
            Ok(config) => Some(config),
            Err(error) => {
                log::error!("configuration change ignored: {error}");
                None
            }
        }
    }
}

pub fn load_initial() -> Result<SkinConfig, String> {
    match read() {
        Ok(config) => Ok(config),
        Err(_) if matches!(fs::metadata(CONFIG_PATH), Err(error) if error.kind() == std::io::ErrorKind::NotFound) =>
        {
            fs::write(CONFIG_PATH, "{}\n")
                .map_err(|error| format!("could not create {CONFIG_PATH}: {error}"))?;
            log::info!("created empty {CONFIG_PATH}");
            Ok(SkinConfig::default())
        }
        Err(error) => Err(error),
    }
}

pub fn skin_source_revision(definition: &SkinDefinition) -> SkinSourceRevision {
    SkinSourceRevision {
        definition: definition.clone(),
        txd: file_revision(&definition.txd_path),
        dff: file_revision(&definition.dff_path),
    }
}

fn read() -> Result<SkinConfig, String> {
    let text = fs::read_to_string(CONFIG_PATH)
        .map_err(|error| format!("could not read {CONFIG_PATH}: {error}"))?;
    parse(&text)
}

fn parse(text: &str) -> Result<SkinConfig, String> {
    let config: SkinConfig =
        serde_json::from_str(text).map_err(|error| format!("invalid {CONFIG_PATH}: {error}"))?;

    for (skin_id, definition) in &config.skins {
        if definition.txd_path.is_empty() || definition.dff_path.is_empty() {
            return Err(format!("skin {skin_id} has an empty asset path"));
        }
        if !(0..20_000).contains(&definition.donor_model_id) {
            return Err(format!(
                "skin {skin_id} has invalid donor_model_id {}",
                definition.donor_model_id
            ));
        }
    }

    Ok(config)
}

fn file_revision(path: &str) -> FileRevision {
    match fs::metadata(path) {
        Ok(metadata) => match metadata.modified() {
            Ok(modified) => FileRevision::Present {
                modified,
                length: metadata.len(),
            },
            Err(error) => FileRevision::Unreadable(error.to_string()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileRevision::Missing,
        Err(error) => FileRevision::Unreadable(error.to_string()),
    }
}
