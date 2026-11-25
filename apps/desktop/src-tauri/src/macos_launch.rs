//! How macOS launched the app. A login item (Apple's `SMAppService.mainApp`)
//! takes no arguments, so a launch at login is told by the "open
//! application" Apple event, which carries `keyAELaunchedAsLogInItem` then.
//! The event is only current while the app finishes launching, which is
//! when Tauri's setup runs.

use objc2_foundation::NSAppleEventManager;

/// Four-character codes from Apple's AppleEvents.h and AERegistry.h.
const OPEN_APPLICATION: u32 = u32::from_be_bytes(*b"oapp");
const PROPERTY_DATA: u32 = u32::from_be_bytes(*b"prop");
const LAUNCHED_AS_LOGIN_ITEM: u32 = u32::from_be_bytes(*b"lgit");

/// Whether macOS opened the app as a login item. False when it can't tell,
/// so a doubtful launch shows the window rather than hide the app.
pub fn was_launched_as_login_item() -> bool {
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    event.eventID() == OPEN_APPLICATION
        && event
            .paramDescriptorForKeyword(PROPERTY_DATA)
            .is_some_and(|property| property.enumCodeValue() == LAUNCHED_AS_LOGIN_ITEM)
}
