//! Files the UI may send, open and save. The UI only ever holds tokens for
//! files the person picked, dropped or pasted, never paths it could make up,
//! so a bug in it can't upload something like `~/.ssh`. Save as asks the
//! person where, from here, for the same reason.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::Serialize;
use shy_core::{
    views::FileInfo,
    workspace::{FileSource, NewFile},
};
use tauri::{
    AppHandle, DragDropEvent, Emitter, State,
    ipc::{InvokeBody, Request, Response},
};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::oneshot;

use crate::{AppState, CommandResult, current, failed};

/// Pasted files go through the bridge as bytes, so they stay small.
const MAX_PASTED_BYTES: usize = 25 * 1024 * 1024;
/// Matches `MAX_INLINE_IMAGE_BYTES` in the UI's `lib/files.ts`.
const MAX_THUMBNAIL_SOURCE_BYTES: u64 = 5 * 1024 * 1024;

/// The source's bytes, if it holds at most `limit` of them. Reads at most
/// one byte past the limit, so a file that grew since it was staged is
/// refused rather than read whole.
fn small_copy(source: FileSource, limit: u64) -> CommandResult<Vec<u8>> {
    let too_large = || "too large for a thumbnail".to_owned();
    match source {
        FileSource::Bytes(bytes) if bytes.len() as u64 <= limit => Ok(bytes),
        FileSource::Bytes(_) => Err(too_large()),
        FileSource::Path(path) => {
            let file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            file.take(limit + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            if bytes.len() as u64 > limit {
                return Err(too_large());
            }
            Ok(bytes)
        }
    }
}

/// Tokens only ever name files the person chose, so guessing one gains
/// nothing; a counter is enough.
#[derive(Default)]
pub struct StagedFiles {
    files: Mutex<HashMap<String, Staged>>,
    next_token: AtomicU64,
}

struct Staged {
    name: String,
    media_type: String,
    source: FileSource,
}

/// What the UI shows for a file waiting to be sent.
#[derive(Serialize, Clone)]
pub struct StagedFile {
    token: String,
    name: String,
    size: u64,
    media_type: String,
}

impl StagedFiles {
    fn add(&self, name: String, media_type: String, size: u64, source: FileSource) -> StagedFile {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed).to_string();
        let staged = StagedFile {
            token: token.clone(),
            name: name.clone(),
            size,
            media_type: media_type.clone(),
        };
        self.lock().insert(
            token,
            Staged {
                name,
                media_type,
                source,
            },
        );
        staged
    }

    fn add_path(&self, path: PathBuf) -> Option<StagedFile> {
        let metadata = std::fs::metadata(&path).ok().filter(std::fs::Metadata::is_file)?;
        let name = path.file_name()?.to_string_lossy().into_owned();
        let media_type = media_type_of(&path).to_owned();
        Some(self.add(name, media_type, metadata.len(), FileSource::Path(path)))
    }

    /// A copy, so the file stays staged if sending fails and is tried again.
    fn get(&self, token: &str) -> CommandResult<NewFile> {
        let files = self.lock();
        let staged = files
            .get(token)
            .ok_or_else(|| "that file is no longer waiting to be sent".to_owned())?;
        Ok(NewFile {
            name: staged.name.clone(),
            media_type: staged.media_type.clone(),
            source: staged.source.clone(),
        })
    }

    /// Lets every staged file go, as when another workspace opens.
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Staged>> {
        self.files
            .lock()
            .expect("staged files lock poisoned by an earlier panic")
    }
}

/// A hint for the receiver; apps decide how to show a file from its bytes.
fn media_type_of(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

/// Stages files dropped on the window and tells the UI about them.
pub fn on_drag_drop(app: &AppHandle, staged: &StagedFiles, event: &DragDropEvent) {
    let message = match event {
        DragDropEvent::Enter { .. } | DragDropEvent::Over { .. } => {
            let _ = app.emit("file-drag", true);
            return;
        }
        DragDropEvent::Leave => {
            let _ = app.emit("file-drag", false);
            return;
        }
        DragDropEvent::Drop { paths, .. } => paths
            .iter()
            .filter_map(|path| staged.add_path(path.clone()))
            .collect::<Vec<_>>(),
        _ => return,
    };
    let _ = app.emit("file-drag", false);
    if !message.is_empty() {
        let _ = app.emit("files-dropped", message);
    }
}

#[tauri::command]
pub async fn pick_files(app: AppHandle, staged: State<'_, StagedFiles>) -> CommandResult<Vec<StagedFile>> {
    let (answer, picked) = oneshot::channel();
    app.dialog().file().pick_files(move |paths| {
        let _ = answer.send(paths);
    });
    let paths = picked
        .await
        .map_err(|_| "the file picker closed unexpectedly".to_owned())?;
    Ok(paths
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .filter_map(|path| staged.add_path(path))
        .collect())
}

/// The UI sends a pasted file as raw bytes (see `framePastedFile` in
/// `lib/api.ts`), so 25 MB doesn't become a JSON array of numbers.
#[tauri::command]
pub fn stage_pasted_file(staged: State<'_, StagedFiles>, request: Request<'_>) -> CommandResult<StagedFile> {
    let InvokeBody::Raw(framed) = request.body() else {
        return Err("a pasted file must arrive as raw bytes".to_owned());
    };
    let (name, media_type, bytes) = unframe_pasted_file(framed)?;
    if bytes.len() > MAX_PASTED_BYTES {
        return Err("that's too large to paste; attach it instead".to_owned());
    }
    let size = u64::try_from(bytes.len()).map_err(|error| error.to_string())?;
    Ok(staged.add(name, media_type, size, FileSource::Bytes(bytes.to_vec())))
}

fn unframe_pasted_file(framed: &[u8]) -> CommandResult<(String, String, &[u8])> {
    let malformed = || "a pasted file arrived malformed".to_owned();
    let mut rest = framed;
    let mut texts = Vec::new();
    for _ in 0..2 {
        let (length, after) = rest.split_first_chunk::<2>().ok_or_else(malformed)?;
        let length = usize::from(u16::from_be_bytes(*length));
        let (text, after) = after.split_at_checked(length).ok_or_else(malformed)?;
        texts.push(String::from_utf8(text.to_vec()).map_err(|_| malformed())?);
        rest = after;
    }
    let media_type = texts.pop().ok_or_else(malformed)?;
    let name = texts.pop().ok_or_else(malformed)?;
    Ok((name, media_type, rest))
}

/// A staged image's bytes, for its thumbnail in the message box. Only small
/// files, so a large one isn't read into memory just to show a corner of it.
/// Runs off the main thread, so a slow disk never freezes the window.
#[tauri::command(async)]
pub fn staged_file_bytes(staged: State<'_, StagedFiles>, token: String) -> CommandResult<Response> {
    let source = staged
        .lock()
        .get(&token)
        .map(|file| match &file.source {
            FileSource::Path(path) => Ok(FileSource::Path(path.clone())),
            FileSource::Bytes(bytes) if bytes.len() as u64 <= MAX_THUMBNAIL_SOURCE_BYTES => {
                Ok(FileSource::Bytes(bytes.clone()))
            }
            FileSource::Bytes(_) => Err("too large for a thumbnail".to_owned()),
        })
        .ok_or_else(|| "that file is no longer waiting to be sent".to_owned())??;
    Ok(Response::new(small_copy(source, MAX_THUMBNAIL_SOURCE_BYTES)?))
}

#[tauri::command]
pub fn unstage_file(staged: State<'_, StagedFiles>, token: String) {
    staged.lock().remove(&token);
}

#[tauri::command]
pub async fn send_files(
    state: State<'_, AppState>,
    staged: State<'_, StagedFiles>,
    channel: String,
    text: String,
    reply_to: Option<String>,
    tokens: Vec<String>,
) -> CommandResult<String> {
    let files = tokens
        .iter()
        .map(|token| staged.get(token))
        .collect::<CommandResult<Vec<_>>>()?;
    let message = current(&state)
        .await?
        .send_files(&channel, &text, reply_to, files)
        .await
        .map_err(failed)?;
    let mut waiting = staged.lock();
    for token in &tokens {
        waiting.remove(token);
    }
    Ok(message)
}

#[tauri::command]
pub async fn share_page_file(
    state: State<'_, AppState>,
    staged: State<'_, StagedFiles>,
    page: String,
    token: String,
) -> CommandResult<String> {
    let file = staged.get(&token)?;
    let id = current(&state)
        .await?
        .share_page_file(&page, file)
        .await
        .map_err(failed)?;
    staged.lock().remove(&token);
    Ok(id)
}

#[tauri::command]
pub async fn page_files(state: State<'_, AppState>, page: String) -> CommandResult<Vec<FileInfo>> {
    current(&state).await?.page_files(&page).map_err(failed)
}

#[tauri::command]
pub async fn page_file_bytes(
    state: State<'_, AppState>,
    page: String,
    file: String,
) -> CommandResult<Response> {
    let bytes = current(&state)
        .await?
        .page_file_bytes(&page, &file)
        .await
        .map_err(failed)?;
    Ok(Response::new(bytes))
}

/// Like [`save_file`], for one of a page's own files.
#[tauri::command]
pub async fn save_page_file(
    app: AppHandle,
    state: State<'_, AppState>,
    page: String,
    file: String,
) -> CommandResult<bool> {
    let workspace = current(&state).await?;
    let name = workspace
        .page_files(&page)
        .map_err(failed)?
        .into_iter()
        .find(|candidate| candidate.id == file)
        .map(|found| found.name)
        .ok_or_else(|| "that file isn't part of this page".to_owned())?;
    let Some(path) = ask_where_to_save(&app, name).await? else {
        return Ok(false);
    };
    workspace
        .save_page_file(&page, &file, &path)
        .await
        .map_err(failed)?;
    Ok(true)
}

#[tauri::command]
pub async fn file_bytes(state: State<'_, AppState>, file: String) -> CommandResult<Response> {
    let bytes = current(&state).await?.file_bytes(&file).await.map_err(failed)?;
    Ok(Response::new(bytes))
}

/// Asks where to save, then saves. False if the person cancelled.
#[tauri::command]
pub async fn save_file(app: AppHandle, state: State<'_, AppState>, file: String) -> CommandResult<bool> {
    let workspace = current(&state).await?;
    let name = workspace
        .files()
        .map_err(failed)?
        .into_iter()
        .find(|candidate| candidate.id == file)
        .map(|found| found.name)
        .ok_or_else(|| "no such file here".to_owned())?;
    let Some(path) = ask_where_to_save(&app, name).await? else {
        return Ok(false);
    };
    workspace.save_file(&file, &path).await.map_err(failed)?;
    Ok(true)
}

/// The system's save dialog, offering `name`; None if the person cancelled.
async fn ask_where_to_save(app: &AppHandle, name: String) -> CommandResult<Option<PathBuf>> {
    let (answer, chosen) = oneshot::channel();
    app.dialog().file().set_file_name(name).save_file(move |path| {
        let _ = answer.send(path);
    });
    let Some(path) = chosen
        .await
        .map_err(|_| "the save dialog closed unexpectedly".to_owned())?
    else {
        return Ok(None);
    };
    Ok(Some(path.into_path().map_err(|error| error.to_string())?))
}

#[tauri::command]
pub async fn delete_file(state: State<'_, AppState>, file: String) -> CommandResult<()> {
    current(&state).await?.delete_file(&file).map_err(failed)
}

#[tauri::command]
pub async fn rename_file(state: State<'_, AppState>, file: String, name: String) -> CommandResult<()> {
    current(&state).await?.rename_file(&file, &name).map_err(failed)
}

#[tauri::command]
pub async fn files(state: State<'_, AppState>) -> CommandResult<Vec<FileInfo>> {
    current(&state).await?.files().map_err(failed)
}

/// A member's picture as raw bytes; empty if they have none.
#[tauri::command]
pub async fn profile_picture(state: State<'_, AppState>, member: String) -> CommandResult<Response> {
    let member = shy_core::views::parse_member_id(&member).map_err(failed)?;
    let picture = current(&state)
        .await?
        .profile_picture(&member)
        .await
        .map_err(failed)?;
    Ok(Response::new(picture.unwrap_or_default()))
}

/// Shares an already cropped picture; empty bytes remove it.
#[tauri::command]
pub async fn set_profile_picture(state: State<'_, AppState>, image: Vec<u8>) -> CommandResult<()> {
    let image = (!image.is_empty()).then_some(image);
    current(&state)
        .await?
        .set_profile_picture(image)
        .await
        .map_err(failed)
}

/// A workspace's icon as raw bytes; empty if it has none. Takes the
/// workspace's id: the rail shows every workspace on this device.
#[tauri::command]
pub async fn workspace_icon(state: State<'_, AppState>, workspace_id: String) -> CommandResult<Response> {
    let workspace = state
        .workspaces
        .find(&workspace_id)
        .ok_or_else(|| "that workspace isn't open".to_owned())?;
    let icon = workspace.workspace_icon().await.map_err(failed)?;
    Ok(Response::new(icon.unwrap_or_default()))
}

/// Sets the shown workspace's icon from an already cropped picture; empty
/// bytes go back to its letter. Admin only.
#[tauri::command]
pub async fn set_workspace_icon(state: State<'_, AppState>, image: Vec<u8>) -> CommandResult<()> {
    let image = (!image.is_empty()).then_some(image);
    current(&state)
        .await?
        .set_workspace_icon(image)
        .await
        .map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pasted_file_unframes_to_its_name_type_and_bytes() {
        let framed = [&[0, 5][..], b"a.png", &[0, 9], b"image/png", b"BYTES"].concat();
        let (name, media_type, bytes) = unframe_pasted_file(&framed).unwrap();
        assert_eq!(
            (name.as_str(), media_type.as_str(), bytes),
            ("a.png", "image/png", &b"BYTES"[..])
        );
    }

    #[test]
    fn a_short_frame_is_refused() {
        assert!(unframe_pasted_file(&[0, 9, b'a']).is_err());
        assert!(unframe_pasted_file(&[0]).is_err());
    }
}
