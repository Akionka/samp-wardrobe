use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant, SystemTime};

pub const CONFIG_PATH: &str = "custom_skin_loader.json";
const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(1);

fn enabled_by_default() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SkinDefinition {
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub txd_path: String,
    pub dff_path: String,
    pub donor_model_id: i32,
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

    fn matches(&self, player_name: &str, server_model_id: i16) -> bool {
        self.enabled
            && self
                .player_name
                .as_deref()
                .is_none_or(|expected| expected == player_name)
            && self
                .server_model_id
                .is_none_or(|expected| expected == server_model_id)
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct SkinConfig {
    #[serde(default)]
    pub skins: HashMap<String, SkinDefinition>,
    #[serde(default)]
    pub rules: Vec<SkinRule>,
}

impl SkinConfig {
    pub fn matching_rule(&self, player_name: &str, server_model_id: i16) -> Option<&SkinRule> {
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

    for (skin_id, definition) in &config.skins {
        if definition.enabled && (definition.txd_path.is_empty() || definition.dff_path.is_empty())
        {
            return Err(format!("skin {skin_id} has an empty asset path"));
        }
        if !(0..20_000).contains(&definition.donor_model_id) {
            return Err(format!(
                "skin {skin_id} has invalid donor_model_id {}",
                definition.donor_model_id
            ));
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
            .is_some_and(|model_id| !(0..20_000).contains(&(model_id as i32)))
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
                        "dff_path": "",
                        "donor_model_id": 7
                    }
                },
                "rules": []
            }"#,
        )
        .unwrap();

        assert!(!config.skins["draft"].enabled);
    }

    #[test]
    fn rules_prefer_combined_then_player_then_model_matches() {
        let mut config = parse(
            r#"{
                "skins": {
                    "combined": { "txd_path": "combined.txd", "dff_path": "combined.dff", "donor_model_id": 7 },
                    "player": { "txd_path": "player.txd", "dff_path": "player.dff", "donor_model_id": 7 },
                    "model": { "txd_path": "model.txd", "dff_path": "model.dff", "donor_model_id": 7 }
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
                .matching_rule("Jacob_Spencer", 67)
                .unwrap()
                .profile_id,
            "combined"
        );
        assert_eq!(
            config.matching_rule("Jacob_Spencer", 7).unwrap().profile_id,
            "player"
        );
        assert_eq!(
            config.matching_rule("Other_Player", 67).unwrap().profile_id,
            "model"
        );

        config.skins.get_mut("combined").unwrap().enabled = false;
        assert_eq!(
            config
                .matching_rule("Jacob_Spencer", 67)
                .unwrap()
                .profile_id,
            "player"
        );
    }
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
