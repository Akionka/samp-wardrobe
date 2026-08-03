use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant, SystemTime};

use crate::{logging::LogLevel, model_ids};

pub const CONFIG_PATH: &str = "wardrobe.json";
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 2_000;
const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(1);
const MIN_POLL_INTERVAL_MS: u64 = 100;
const MAX_POLL_INTERVAL_MS: u64 = 60_000;

fn enabled_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SkinDefinition {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub txd_path: String,
    pub dff_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SkinRule {
    pub profile_id: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub player_name: Option<String>,
    #[serde(default)]
    pub server_model_id: Option<i16>,
}

impl SkinRule {
    fn priority(&self) -> u8 {
        match (self.player_name.is_some(), self.server_model_id.is_some()) {
            (true, true) => 3,
            (true, false) => 2,
            (false, true) => 1,
            (false, false) => 0,
        }
    }

    fn matches(&self, player_name: Option<&str>, server_model_id: i16) -> bool {
        self.enabled
            && match self.player_name.as_deref() {
                Some(expected) => player_name == Some(expected),
                None => true,
            }
            && self
                .server_model_id
                .is_none_or(|expected| expected == server_model_id)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SkinConfig {
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub log_level: LogLevel,
    #[serde(default)]
    pub skins: HashMap<String, SkinDefinition>,
    #[serde(default)]
    pub rules: Vec<SkinRule>,
}

fn default_poll_interval_ms() -> u64 {
    DEFAULT_POLL_INTERVAL_MS
}

impl Default for SkinConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            log_level: LogLevel::default(),
            skins: HashMap::new(),
            rules: Vec::new(),
        }
    }
}

impl SkinConfig {
    pub fn poll_interval(&self) -> Duration {
        Duration::from_millis(self.poll_interval_ms)
    }

    pub fn matching_rule(
        &self,
        player_name: Option<&str>,
        server_model_id: i16,
    ) -> Option<&SkinRule> {
        (1..=3).rev().find_map(|priority| {
            self.rules.iter().find(|rule| {
                rule.priority() == priority
                    && rule.matches(player_name, server_model_id)
                    && self
                        .skins
                        .get(&rule.profile_id)
                        .is_some_and(|profile| profile.enabled)
            })
        })
    }
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

    if !(MIN_POLL_INTERVAL_MS..=MAX_POLL_INTERVAL_MS).contains(&config.poll_interval_ms) {
        return Err(format!(
            "poll_interval_ms must be between {MIN_POLL_INTERVAL_MS} and {MAX_POLL_INTERVAL_MS}"
        ));
    }

    for (skin_id, definition) in &config.skins {
        if definition.enabled && (definition.txd_path.is_empty() || definition.dff_path.is_empty())
        {
            return Err(format!("skin {skin_id} has an empty asset path"));
        }
    }

    for (index, rule) in config.rules.iter().enumerate() {
        if rule.profile_id.is_empty() || !config.skins.contains_key(&rule.profile_id) {
            return Err(format!("rule {index} refers to an unknown skin profile"));
        }
        if rule.player_name.as_deref().is_some_and(str::is_empty) {
            return Err(format!("rule {index} has an empty player name"));
        }
        if rule
            .server_model_id
            .is_some_and(|model_id| !model_ids::is_valid_model_id(model_id as i32))
        {
            return Err(format!("rule {index} has an invalid server model ID"));
        }
        if rule.priority() == 0 {
            return Err(format!(
                "rule {index} needs a player name or server model ID"
            ));
        }
        if config.rules[..index].iter().any(|previous| {
            previous.player_name == rule.player_name
                && previous.server_model_id == rule.server_model_id
        }) {
            return Err(format!("rule {index} duplicates an earlier rule"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_profiles_accept_empty_asset_paths() {
        let config = parse(
            r#"{
                "skins": {
                    "draft": {
                        "enabled": false,
                        "txd_path": "",
                        "dff_path": ""
                    }
                },
                "rules": []
            }"#,
        )
        .unwrap();

        assert!(!config.skins["draft"].enabled);
    }

    #[test]
    fn defaults_runtime_settings_for_existing_configurations() {
        let config = parse("{}").unwrap();

        assert_eq!(config.poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);
        assert_eq!(config.log_level, LogLevel::Info);
    }

    #[test]
    fn rejects_an_unsafe_complete_scan_interval() {
        let error = parse(r#"{ "poll_interval_ms": 99 }"#).unwrap_err();

        assert!(error.contains("poll_interval_ms must be between 100 and 60000"));
    }

    #[test]
    fn donor_free_profiles_and_legacy_donor_profiles_are_equivalent() {
        let donor_free = parse(
            r#"{
                "skins": {
                    "staff": {
                        "txd_path": "staff.txd",
                        "dff_path": "staff.dff"
                    }
                },
                "rules": []
            }"#,
        )
        .unwrap();
        let legacy = parse(
            r#"{
                "skins": {
                    "staff": {
                        "txd_path": "staff.txd",
                        "dff_path": "staff.dff",
                        "donor_model_id": 18000
                    }
                },
                "rules": []
            }"#,
        )
        .unwrap();

        assert_eq!(donor_free.skins["staff"], legacy.skins["staff"]);
        assert_eq!(
            skin_source_revision(&donor_free.skins["staff"]),
            skin_source_revision(&legacy.skins["staff"])
        );
    }

    #[test]
    fn rules_prefer_combined_then_player_then_model_matches() {
        let mut config = parse(
            r#"{
                "skins": {
                    "combined": { "txd_path": "combined.txd", "dff_path": "combined.dff" },
                    "player": { "txd_path": "player.txd", "dff_path": "player.dff" },
                    "model": { "txd_path": "model.txd", "dff_path": "model.dff" }
                },
                "rules": [
                    { "profile_id": "model", "server_model_id": 67 },
                    { "profile_id": "player", "player_name": "Jacob_Spencer" },
                    { "profile_id": "combined", "player_name": "Jacob_Spencer", "server_model_id": 67 }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            config
                .matching_rule(Some("Jacob_Spencer"), 67)
                .unwrap()
                .profile_id,
            "combined"
        );
        assert_eq!(
            config
                .matching_rule(Some("Jacob_Spencer"), 7)
                .unwrap()
                .profile_id,
            "player"
        );
        assert_eq!(
            config
                .matching_rule(Some("Other_Player"), 67)
                .unwrap()
                .profile_id,
            "model"
        );

        assert_eq!(config.matching_rule(None, 67).unwrap().profile_id, "model");
        assert!(config.matching_rule(None, 7).is_none());

        config.skins.get_mut("combined").unwrap().enabled = false;
        assert_eq!(
            config
                .matching_rule(Some("Jacob_Spencer"), 67)
                .unwrap()
                .profile_id,
            "player"
        );
    }
}
