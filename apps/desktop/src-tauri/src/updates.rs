//! App updates, through Tauri's updater: found and downloaded in the
//! background, and installed only when the person restarts for it, never
//! during a huddle and never before unsent changes are uploaded. The
//! updater checks each package's signature against the key in
//! `tauri.conf.json` before installing it.

use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use shy_core::Workspace;

use crate::{AppState, CommandResult, open_workspaces, shut_down, tray};

/// The update this run found, from download to restart.
#[derive(Default)]
pub struct PendingUpdate {
    stage: Mutex<Option<Stage>>,
    /// Held through a check, so checks that overlap download once.
    checking: tokio::sync::Mutex<()>,
}

enum Stage {
    Downloaded(Box<Downloaded>),
    /// Installed over the app on disk, but a huddle started meanwhile: the
    /// next restart (or quit) runs the new version.
    Installed {
        version: String,
    },
}

struct Downloaded {
    update: Update,
    bytes: Vec<u8>,
}

impl PendingUpdate {
    fn slot(&self) -> MutexGuard<'_, Option<Stage>> {
        self.stage
            .lock()
            .expect("the pending update's lock is never held across a panic")
    }

    fn version(&self) -> Option<String> {
        match self.slot().as_ref()? {
            Stage::Downloaded(downloaded) => Some(downloaded.update.version.clone()),
            Stage::Installed { version } => Some(version.clone()),
        }
    }

    /// Runs `check` unless an update is already here; a check that starts
    /// while another runs waits for it and takes what it found.
    async fn check_once<Checked>(&self, check: impl FnOnce() -> Checked) -> CommandResult<Option<String>>
    where
        Checked: Future<Output = CommandResult<Option<Stage>>>,
    {
        let _checking = self.checking.lock().await;
        if let Some(version) = self.version() {
            return Ok(Some(version));
        }
        if let Some(found) = check().await? {
            self.slot().get_or_insert(found);
        }
        Ok(self.version())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartOutcome {
    Restarting,
    /// Updating would end the call; it waits until the huddle is over.
    InHuddle,
    NothingReady,
}

/// Looks for a newer version and downloads it; returns its version once it's
/// ready to install, `None` while there's nothing newer. Development builds
/// never update: installing would replace the build being worked on.
#[tauri::command]
pub async fn check_for_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> CommandResult<Option<String>> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    pending
        .check_once(|| async {
            let updater = app.updater().map_err(|error| error.to_string())?;
            let Some(update) = updater.check().await.map_err(|error| error.to_string())? else {
                return Ok(None);
            };
            let bytes = update
                .download(|_, _| {}, || {})
                .await
                .map_err(|error| error.to_string())?;
            Ok(Some(Stage::Downloaded(Box::new(Downloaded { update, bytes }))))
        })
        .await
}

/// Installs the downloaded update and restarts into it, after uploading what
/// wasn't sent yet.
#[tauri::command]
pub async fn restart_to_update(
    app: AppHandle,
    state: State<'_, AppState>,
    pending: State<'_, PendingUpdate>,
) -> CommandResult<RestartOutcome> {
    if is_in_huddle(&state).await {
        return Ok(RestartOutcome::InHuddle);
    }
    let Some(stage) = pending.slot().take() else {
        return Ok(RestartOutcome::NothingReady);
    };
    let downloaded = match stage {
        Stage::Downloaded(downloaded) => downloaded,
        Stage::Installed { version } => {
            *pending.slot() = Some(Stage::Installed { version });
            return restart_unless_in_huddle(&app, &state).await;
        }
    };
    let version = downloaded.update.version.clone();
    if cfg!(windows) {
        // The plugin's Windows install starts the installer and ends this
        // process at once, so unsent changes go first.
        let Ok(workspaces) = take_unless_in_huddle(&state).await else {
            *pending.slot() = Some(Stage::Downloaded(downloaded));
            return Ok(RestartOutcome::InHuddle);
        };
        shut_down(workspaces).await;
        if let Err(error) = install(&pending, downloaded).await {
            tracing::warn!("the update didn't install: {error}");
            tray::restore_after_cancelled_exit(&app);
            open_workspaces(&app, &state).await?;
            return Err(error);
        }
    } else {
        install(&pending, downloaded).await?;
    }
    *pending.slot() = Some(Stage::Installed { version });
    restart_unless_in_huddle(&app, &state).await
}

/// Installs over the app on disk, off the async threads: it unpacks tens of
/// megabytes and, for a `.deb` or `.rpm`, waits on an administrator prompt.
/// A failed install keeps the download for another try.
async fn install(pending: &PendingUpdate, downloaded: Box<Downloaded>) -> CommandResult<()> {
    let (downloaded, installed) = tauri::async_runtime::spawn_blocking(move || {
        let installed = downloaded.update.install(&downloaded.bytes);
        (downloaded, installed)
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err(error) = installed {
        pending.slot().get_or_insert(Stage::Downloaded(downloaded));
        return Err(error.to_string());
    }
    Ok(())
}

/// The install can take a while (a password prompt, say), so a huddle may
/// have started since the person asked; it isn't cut off.
async fn restart_unless_in_huddle(app: &AppHandle, state: &AppState) -> CommandResult<RestartOutcome> {
    let Ok(workspaces) = take_unless_in_huddle(state).await else {
        return Ok(RestartOutcome::InHuddle);
    };
    shut_down(workspaces).await;
    app.request_restart();
    Ok(RestartOutcome::Restarting)
}

async fn is_in_huddle(state: &AppState) -> bool {
    state
        .open_workspaces()
        .iter()
        .any(|workspace| workspace.my_huddle().is_some())
}

struct InHuddle;

/// Takes the open workspaces out of the app, so nothing else starts on
/// them, unless the person is in a huddle in one.
async fn take_unless_in_huddle(state: &AppState) -> Result<Vec<Workspace>, InHuddle> {
    if is_in_huddle(state).await {
        return Err(InHuddle);
    }
    Ok(state.workspaces.close_all())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[tokio::test]
    async fn overlapping_checks_download_once() {
        let pending = PendingUpdate::default();
        let downloads = AtomicUsize::new(0);
        let check = || async {
            downloads.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(Some(Stage::Installed {
                version: "0.2.0".into(),
            }))
        };

        let (first, second) = tokio::join!(pending.check_once(check), pending.check_once(check));

        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        assert_eq!(first.unwrap().as_deref(), Some("0.2.0"));
        assert_eq!(second.unwrap().as_deref(), Some("0.2.0"));
    }

    #[tokio::test]
    async fn a_check_that_finds_nothing_lets_the_next_one_look_again() {
        let pending = PendingUpdate::default();
        let found = pending.check_once(|| async { Ok(None) }).await;
        assert_eq!(found.unwrap(), None);

        let found = pending
            .check_once(|| async {
                Ok(Some(Stage::Installed {
                    version: "0.2.0".into(),
                }))
            })
            .await;
        assert_eq!(found.unwrap().as_deref(), Some("0.2.0"));
    }
}
