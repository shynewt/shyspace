//! Notifications on macOS through Apple's UNUserNotificationCenter: it
//! reports whether the person allowed them (Tauri's plugin can't), asks
//! once, and offers quick actions. It only works in a `.app` bundle with an
//! identifier; `is_available` says when, and `alerts` falls back to the
//! plugin otherwise (as in development builds).
//!
//! The callbacks Apple runs (completion handlers, the delegate) may come on
//! any thread, so they only pass plain Rust values on.

// The one exception to the crate's `deny(unsafe_code)`: calling Objective-C
// needs it. Every `unsafe` block below is small and says why it's sound.
#![allow(unsafe_code)]

use std::ptr::NonNull;
use std::sync::{Mutex, PoisonError};

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString, NSURL};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent, UNNotification,
    UNNotificationAction, UNNotificationActionOptions, UNNotificationAttachment, UNNotificationCategory,
    UNNotificationCategoryOptions, UNNotificationDefaultActionIdentifier,
    UNNotificationDismissActionIdentifier, UNNotificationPresentationOptions, UNNotificationRequest,
    UNNotificationResponse, UNNotificationSettings, UNNotificationSound, UNTextInputNotificationAction,
    UNTextInputNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use tokio::sync::oneshot;

use crate::alert_actions::{AlertCategory, MARK_READ_ACTION, OPEN_ACTION, Permission, REPLY_ACTION};

/// Gets each response: the notification's identifier, the action
/// (`OPEN_ACTION` for a click), and any text typed into it.
type OnResponse = Box<dyn Fn(String, String, Option<String>) + Send + Sync>;

/// Whether this process may use the notification center: Apple raises an
/// exception outside a `.app` bundle with an identifier. A development
/// build has an identifier (embedded) but no bundle, so both are checked.
pub fn is_available() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

/// Registers the quick actions and starts passing responses to
/// `on_response`. Call once, on the main thread, before the event loop
/// runs, so a response that launched the app is delivered.
pub fn start(on_response: impl Fn(String, String, Option<String>) + Send + Sync + 'static) {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.setNotificationCategories(&categories());
    let delegate = Delegate::new(Box::new(on_response));
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    // The center holds its delegate weakly, and this one serves for the
    // life of the app, so its reference is never released.
    std::mem::forget(delegate);
}

fn categories() -> Retained<NSSet<UNNotificationCategory>> {
    let categories: Vec<_> = AlertCategory::ALL
        .into_iter()
        .map(|category| {
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(category.id()),
                &NSArray::from_retained_slice(&actions_of(category)),
                &NSArray::new(),
                UNNotificationCategoryOptions::empty(),
            )
        })
        .collect();
    NSSet::from_retained_slice(&categories)
}

fn actions_of(category: AlertCategory) -> Vec<Retained<UNNotificationAction>> {
    let mark_read = UNNotificationAction::actionWithIdentifier_title_options(
        &NSString::from_str(MARK_READ_ACTION),
        &NSString::from_str("Mark as read"),
        UNNotificationActionOptions::empty(),
    );
    match category {
        AlertCategory::Message => {
            let reply =
                UNTextInputNotificationAction::actionWithIdentifier_title_options_textInputButtonTitle_textInputPlaceholder(
                    &NSString::from_str(REPLY_ACTION),
                    &NSString::from_str("Reply"),
                    UNNotificationActionOptions::empty(),
                    &NSString::from_str("Send"),
                    &NSString::from_str("Reply"),
                );
            vec![Retained::into_super(reply), mark_read]
        }
        AlertCategory::Page => vec![mark_read],
    }
}

/// Shows a notification now. `picture` is a PNG file, which the system
/// moves into its own store; a picture it refuses leaves it out.
pub fn show(
    identifier: &str,
    title: &str,
    body: &str,
    category: Option<AlertCategory>,
    picture: Option<&str>,
) -> Result<(), String> {
    let content = UNMutableNotificationContent::new();
    content.setTitle(&NSString::from_str(title));
    content.setBody(&NSString::from_str(body));
    content.setSound(Some(&UNNotificationSound::defaultSound()));
    if let Some(category) = category {
        content.setCategoryIdentifier(&NSString::from_str(category.id()));
    }
    if let Some(path) = picture {
        match attachment(path) {
            Ok(attachment) => content.setAttachments(&NSArray::from_retained_slice(&[attachment])),
            Err(error) => tracing::warn!("an alert's picture was left out: {error}"),
        }
    }
    let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
        &NSString::from_str(identifier),
        &content,
        None,
    );
    let on_added = RcBlock::new(|error: *mut NSError| {
        if let Some(error) = error_text(error) {
            tracing::warn!("macOS didn't show an alert: {error}");
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .addNotificationRequest_withCompletionHandler(&request, Some(&on_added));
    Ok(())
}

fn attachment(path: &str) -> Result<Retained<UNNotificationAttachment>, String> {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    // SAFETY: no options dictionary is passed, so no option can have the wrong type.
    let attached = unsafe {
        UNNotificationAttachment::attachmentWithIdentifier_URL_options_error(
            &NSString::from_str("picture"),
            &url,
            None,
        )
    };
    attached.map_err(|error| error.localizedDescription().to_string())
}

/// The text of an error a completion handler got, if it got one.
fn error_text(error: *mut NSError) -> Option<String> {
    // SAFETY: a completion handler's error is null or a valid NSError that
    // lives at least until the handler returns, which outlasts this call.
    let error = unsafe { error.as_ref() }?;
    Some(error.localizedDescription().to_string())
}

pub async fn permission() -> Result<Permission, String> {
    let status = ask_for_settings()
        .await
        .map_err(|_| "macOS didn't say whether shyspace may notify".to_owned())?;
    Ok(permission_of(status))
}

/// Asks for the notification settings; the answer comes on the receiver.
/// Not async itself, so no Objective-C object is held across an `.await`
/// (they aren't `Send`); Apple keeps its own copy of the block.
fn ask_for_settings() -> oneshot::Receiver<UNAuthorizationStatus> {
    let (sender, receiver) = oneshot::channel();
    let sender = Mutex::new(Some(sender));
    let on_settings = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        // SAFETY: Apple passes a valid settings object that lives until the
        // handler returns; it's only read here.
        let status = unsafe { settings.as_ref() }.authorizationStatus();
        if let Some(sender) = sender.lock().unwrap_or_else(PoisonError::into_inner).take() {
            // Nobody is waiting any more if this fails; nothing to do then.
            sender.send(status).ok();
        }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .getNotificationSettingsWithCompletionHandler(&on_settings);
    receiver
}

fn permission_of(status: UNAuthorizationStatus) -> Permission {
    match status {
        UNAuthorizationStatus::NotDetermined => Permission::NotAsked,
        UNAuthorizationStatus::Denied => Permission::Denied,
        _ => Permission::Allowed,
    }
}

/// Shows macOS's prompt if it hasn't been answered, then reads the answer.
pub async fn request_permission() -> Result<Permission, String> {
    ask_to_notify()
        .await
        .map_err(|_| "macOS didn't answer the request to notify".to_owned())?;
    permission().await
}

/// Like `ask_for_settings`, for the permission prompt.
fn ask_to_notify() -> oneshot::Receiver<()> {
    let (sender, receiver) = oneshot::channel();
    let sender = Mutex::new(Some(sender));
    let on_answer = RcBlock::new(move |_granted: Bool, error: *mut NSError| {
        if let Some(error) = error_text(error) {
            tracing::warn!("asking to notify failed: {error}");
        }
        if let Some(sender) = sender.lock().unwrap_or_else(PoisonError::into_inner).take() {
            sender.send(()).ok();
        }
    });
    UNUserNotificationCenter::currentNotificationCenter().requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
        &on_answer,
    );
    receiver
}

/// What a response asks for, as plain values; None for a dismissal.
fn read_response(response: &UNNotificationResponse) -> Option<(String, String, Option<String>)> {
    let action = response.actionIdentifier();
    // SAFETY: these are constant NSStrings the framework defines; reading
    // an extern static is unsafe only because Rust can't see that.
    let (open, dismiss) = unsafe {
        (
            UNNotificationDefaultActionIdentifier,
            UNNotificationDismissActionIdentifier,
        )
    };
    if &*action == dismiss {
        return None;
    }
    let action = if &*action == open {
        OPEN_ACTION.to_owned()
    } else {
        action.to_string()
    };
    let identifier = response.notification().request().identifier().to_string();
    let typed = response
        .downcast_ref::<UNTextInputNotificationResponse>()
        .map(|typed| typed.userText().to_string());
    Some((identifier, action, typed))
}

struct DelegateIvars {
    on_response: OnResponse,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Delegate`
    // doesn't implement `Drop`. Its only state is `Send + Sync`, so it may
    // be called on any thread.
    #[unsafe(super(NSObject))]
    #[name = "ShyspaceNotificationDelegate"]
    #[ivars = DelegateIvars]
    struct Delegate;

    // SAFETY: NSObjectProtocol asks nothing more of a subclass of NSObject.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: each method's selector and types are the protocol's, as the
    // bindings declare them, and each calls its completion handler once.
    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        /// Shows banners while shyspace is in front too; the frontend
        /// already holds back an alert for what you're looking at.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::List
                | UNNotificationPresentationOptions::Sound,));
        }

        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion_handler: &DynBlock<dyn Fn()>,
        ) {
            if let Some((identifier, action, typed)) = read_response(response) {
                // A panic must not unwind into Apple's frames.
                let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    (self.ivars().on_response)(identifier, action, typed);
                }));
                if handled.is_err() {
                    tracing::error!("handling a response to an alert panicked");
                }
            }
            completion_handler.call(());
        }
    }
);

impl Delegate {
    fn new(on_response: OnResponse) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DelegateIvars { on_response });
        // SAFETY: NSObject's `init` takes no arguments and returns the
        // initialized object.
        unsafe { msg_send![super(this), init] }
    }
}
