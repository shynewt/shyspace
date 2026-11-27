//! Android setup the core needs before its first connection.

use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JObject};
use jni::{EnvUnowned, jni_mangle};

/// Lets the core's HTTPS requests (to the bucket, and some of iroh's) check certificates against
/// the phone's trust store. Must run once, before the first connection: until then, checking a
/// certificate panics.
#[jni_mangle("dev.shynewt.shyspace.core.PlatformTls")]
pub fn use_phone_trust_store<'caller>(
    mut env: EnvUnowned<'caller>,
    _class: JClass<'caller>,
    context: JObject<'caller>,
) {
    env.with_env(|env| rustls_platform_verifier::android::init_with_env(env, context))
        .resolve::<ThrowRuntimeExAndDefault>();
}
