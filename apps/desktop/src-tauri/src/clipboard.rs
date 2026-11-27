//! The system clipboard, read only when the person asks: its image when
//! they paste and the web view handed the page nothing (some WebKitGTK
//! builds drop pasted images on the way to the page), and its text for a
//! Paste button, which the web view's own clipboard API can't read.

use tauri::{AppHandle, ipc::Response};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::CommandResult;

/// Room for an 8K screenshot's raw pixels; the PNG made from them is
/// checked against the paste limit afterwards.
const MAX_IMAGE_BYTES: usize = 144 * 1024 * 1024;
const TOO_LARGE: &str = "the clipboard's image is too large to paste. Attach it instead.";

/// The clipboard's image as its width and height (little-endian u32 each)
/// followed by its RGBA pixels. Runs off the main thread: reading can block,
/// and an image this app copied is served from the main loop.
#[tauri::command(async)]
pub fn clipboard_image(app: AppHandle) -> CommandResult<Response> {
    let image = app
        .clipboard()
        .read_image()
        .map_err(|error| format!("there's no image on the clipboard to paste ({error})"))?;
    Ok(Response::new(frame_image(
        image.width(),
        image.height(),
        image.rgba(),
    )?))
}

/// Longer than any code a person pastes into the app.
const MAX_TEXT_BYTES: usize = 64 * 1024;

/// The clipboard's text, for a Paste button.
#[tauri::command(async)]
pub fn clipboard_text(app: AppHandle) -> CommandResult<String> {
    let text = app
        .clipboard()
        .read_text()
        .map_err(|_| "There's no text on the clipboard to paste.".to_owned())?;
    if text.len() > MAX_TEXT_BYTES {
        return Err("the clipboard's text is too long to be a code".to_owned());
    }
    Ok(text)
}

fn frame_image(width: u32, height: u32, rgba: &[u8]) -> CommandResult<Vec<u8>> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(TOO_LARGE)?;
    if expected > MAX_IMAGE_BYTES {
        return Err(TOO_LARGE.to_owned());
    }
    if expected == 0 || rgba.len() != expected {
        return Err("the clipboard's image is damaged".to_owned());
    }
    let mut framed = Vec::with_capacity(8 + rgba.len());
    framed.extend_from_slice(&width.to_le_bytes());
    framed.extend_from_slice(&height.to_le_bytes());
    framed.extend_from_slice(rgba);
    Ok(framed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_image_frames_as_its_size_then_its_pixels() {
        let pixels = [1, 2, 3, 4, 5, 6, 7, 8];
        let framed = frame_image(2, 1, &pixels).unwrap();
        assert_eq!(framed, [&[2, 0, 0, 0, 1, 0, 0, 0][..], &pixels].concat());
    }

    #[test]
    fn pixels_that_dont_match_the_size_are_refused() {
        assert!(frame_image(2, 2, &[0; 8]).is_err());
        assert!(frame_image(0, 0, &[]).is_err());
    }

    #[test]
    fn an_image_beyond_the_limit_is_refused() {
        assert!(frame_image(8192, 8192, &[]).unwrap_err().contains("too large"));
        assert!(
            frame_image(u32::MAX, u32::MAX, &[])
                .unwrap_err()
                .contains("too large")
        );
    }
}
