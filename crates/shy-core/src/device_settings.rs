//! This device's settings that aren't any one workspace's (`workspaces.json`):
//! the workspaces' order, recency, mutes and pause, and the huddle devices and
//! sounds.
//! Shared by the workspace manager and the workspaces it opens.

use std::{path::PathBuf, sync::Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::huddle::media::DeviceChoice;

/// This device's choices about its workspaces.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct DeviceSettings {
    /// The order they're shown in.
    #[serde(default)]
    pub(crate) workspace_ids: Vec<String>,
    /// Most recently selected first.
    #[serde(default)]
    pub(crate) recent: Vec<String>,
    #[serde(default)]
    pub(crate) muted: Vec<String>,
    #[serde(default)]
    pub(crate) notifications_paused_until_ms: Option<i64>,
    /// Workspaces whose folder is being deleted: a leave cut short is
    /// finished from here.
    #[serde(default)]
    pub(crate) leaving: Vec<String>,
    /// The single folder of earlier versions was moved (or there was none),
    /// so start-up needn't look for it again.
    #[serde(default)]
    pub(crate) is_single_folder_handled: bool,
    /// The microphone and speaker huddles use, whichever workspace calls.
    #[serde(default)]
    pub(crate) huddle_devices: Option<DeviceChoice>,
    /// Whether huddles play join and leave sounds on this device.
    #[serde(default)]
    pub(crate) huddle_sounds: Option<bool>,
    /// A menu bar item on macOS, beside the Dock icon.
    #[serde(default)]
    pub(crate) is_in_menu_bar: Option<bool>,
    /// The unread count on the Dock or taskbar icon.
    #[serde(default)]
    pub(crate) shows_unread_count: Option<bool>,
    /// The tray icon as a one-colour glyph, on Linux and Windows.
    #[serde(default)]
    pub(crate) is_tray_monochrome: Option<bool>,
}

pub(crate) struct DeviceSettingsFile {
    path: PathBuf,
    /// Held across each change, so two can't interleave.
    changing: Mutex<()>,
}

impl DeviceSettingsFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            changing: Mutex::default(),
        }
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut DeviceSettings)) -> Result<()> {
        let _changing = self
            .changing
            .lock()
            .expect("settings lock poisoned by an earlier panic");
        let mut settings = self.read()?;
        change(&mut settings);
        self.save(&settings)
    }

    /// A file that can't be read is reported and treated as empty: it holds
    /// only choices, and losing those is better than opening no workspace.
    pub(crate) fn read(&self) -> Result<DeviceSettings> {
        match std::fs::read(&self.path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                tracing::error!("{} is unreadable, so it's ignored: {error}", self.path.display());
                DeviceSettings::default()
            })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DeviceSettings::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    /// Written whole to a new file and renamed over the old one, so a crash
    /// leaves either version, never half of one.
    fn save(&self, settings: &DeviceSettings) -> Result<()> {
        let path = &self.path;
        let partial = PathBuf::from(format!("{}.partial", path.display()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file =
            std::fs::File::create(&partial).with_context(|| format!("writing {}", partial.display()))?;
        std::io::Write::write_all(&mut file, &serde_json::to_vec_pretty(settings)?)
            .with_context(|| format!("writing {}", partial.display()))?;
        file.sync_all()
            .with_context(|| format!("writing {}", partial.display()))?;
        std::fs::rename(&partial, path).with_context(|| format!("writing {}", path.display()))
    }

    pub(crate) fn huddle_devices(&self) -> Result<Option<DeviceChoice>> {
        Ok(self.read()?.huddle_devices)
    }

    pub(crate) fn set_huddle_devices(&self, choice: &DeviceChoice) -> Result<()> {
        self.update(|settings| settings.huddle_devices = Some(choice.clone()))
    }

    pub(crate) fn huddle_sounds(&self) -> Result<Option<bool>> {
        Ok(self.read()?.huddle_sounds)
    }

    pub(crate) fn set_huddle_sounds(&self, is_on: bool) -> Result<()> {
        self.update(|settings| settings.huddle_sounds = Some(is_on))
    }
}
