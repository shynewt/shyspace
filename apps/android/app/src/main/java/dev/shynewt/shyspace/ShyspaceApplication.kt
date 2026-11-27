package dev.shynewt.shyspace

import dev.shynewt.shyspace.features.onboarding.LinkKeeper
import dev.shynewt.shyspace.features.onboarding.CorePhoneLinking
import android.app.Application
import dev.shynewt.shyspace.core.AndroidDeviceKeyStore
import dev.shynewt.shyspace.core.KeystoreSealer
import dev.shynewt.shyspace.core.PlatformTls
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.features.settings.AppSettings
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import uniffi.shy_core.Networking
import uniffi.shy_ffi.ShyApp

class ShyspaceApplication : Application() {
    val core: ShyCore by lazy {
        PlatformTls.usePhoneTrustStore(this)
        val keys = AndroidDeviceKeyStore(noBackupFilesDir.resolve("device-keys"), KeystoreSealer())
        ShyCore(ShyApp(noBackupFilesDir.resolve("workspaces").path, keys, Networking.ONLINE))
    }

    /** Linking this phone, beyond the link screen: the wait to be added, and resuming after a restart. */
    val linkKeeper: LinkKeeper by lazy { LinkKeeper(CorePhoneLinking(core.app), CoroutineScope(SupervisorJob() + Dispatchers.Main)) }

    val settings: AppSettings by lazy {
        AppSettings(noBackupFilesDir.resolve("settings.preferences_pb"), CoroutineScope(SupervisorJob() + Dispatchers.IO))
    }
}
