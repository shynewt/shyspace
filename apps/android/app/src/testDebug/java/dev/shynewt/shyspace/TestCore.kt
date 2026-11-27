package dev.shynewt.shyspace

import java.util.concurrent.CopyOnWriteArrayList
import com.sun.jna.Native
import dev.shynewt.shyspace.core.ShyCore
import java.io.File
import uniffi.shy_core.Networking
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_ffi.DeviceKeyStore
import uniffi.shy_ffi.DeviceKeyStoreException
import uniffi.shy_ffi.ShyApp
import uniffi.shy_ffi.ShyWorkspace

/** Keeps device keys in memory, holding to the core's contract. */
class MemoryKeyStore : DeviceKeyStore {
    val secrets = mutableMapOf<String, ByteArray>()

    override fun read(account: String): ByteArray? = synchronized(secrets) { secrets[account] }

    override fun create(account: String, secret: ByteArray) = synchronized(secrets) {
        if (secrets.containsKey(account)) throw DeviceKeyStoreException.Failed("$account already has a secret")
        secrets[account] = secret
    }

    override fun delete(account: String) {
        synchronized(secrets) { secrets.remove(account) }
    }
}

/** A real core in [folder], offline, on a bucket kept in memory; pass [keys] again to start it anew on the same phone. */
fun testCore(folder: File, keys: MemoryKeyStore = MemoryKeyStore()): ShyCore {
    CoreCallbackCrashes.failIfAny()
    return ShyCore(ShyApp.withMemoryBucket(folder.path, keys, Networking.OFFLINE))
}

/**
 * Crashes in the core's callbacks into Kotlin, which JNA would only log. One usually means work
 * left running after its test, resuming on a main thread that test already tore down; the next
 * test to make a core, or the main-thread rule, fails with it.
 */
object CoreCallbackCrashes {
    private val crashes = CopyOnWriteArrayList<Throwable>()

    init {
        Native.setCallbackExceptionHandler { _, crash -> crashes += crash }
    }

    fun failIfAny() {
        val crash = crashes.firstOrNull() ?: return
        crashes.clear()
        throw AssertionError("a core callback crashed, as it would on a phone; was work left running after a test?", crash)
    }
}

fun unusedBucket() = BucketSettings("http://unused", "us-east-1", "unused", "unused", "unused-secret")

suspend fun ShyCore.createAcme(): ShyWorkspace = app.createWorkspace("Acme", "Ada", unusedBucket())
