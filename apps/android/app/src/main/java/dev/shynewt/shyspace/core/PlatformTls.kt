package dev.shynewt.shyspace.core

import android.content.Context

/** The core's HTTPS on Android: it checks certificates through the phone's trust store. */
object PlatformTls {
    init {
        System.loadLibrary("shy_ffi")
    }

    /** Must run before the core's first connection; running it again does nothing. */
    @JvmStatic
    external fun usePhoneTrustStore(context: Context)
}
