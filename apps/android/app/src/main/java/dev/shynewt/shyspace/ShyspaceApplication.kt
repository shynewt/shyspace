package dev.shynewt.shyspace

import android.app.Application
import dev.shynewt.shyspace.core.AndroidDeviceKeyStore
import dev.shynewt.shyspace.core.KeystoreSealer
import dev.shynewt.shyspace.core.ShyCore
import uniffi.shy_core.Networking
import uniffi.shy_ffi.ShyApp

class ShyspaceApplication : Application() {
    val core: ShyCore by lazy {
        val keys = AndroidDeviceKeyStore(noBackupFilesDir.resolve("device-keys"), KeystoreSealer())
        ShyCore(ShyApp(noBackupFilesDir.resolve("workspaces").path, keys, Networking.ONLINE))
    }
}
