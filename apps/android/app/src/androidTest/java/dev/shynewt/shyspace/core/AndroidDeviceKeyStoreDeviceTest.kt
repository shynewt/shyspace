package dev.shynewt.shyspace.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.shy_ffi.DeviceKeyStoreException

/**
 * The key store on a real Android system: its file rules (SELinux refuses some calls the JVM
 * allows) and the hardware-backed Keystore.
 */
@RunWith(AndroidJUnit4::class)
class AndroidDeviceKeyStoreDeviceTest {
    private lateinit var folder: File
    private val key = TestSealerKey()

    @Before
    fun makeFolder() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        folder = context.noBackupFilesDir.resolve("device-key-test-${System.nanoTime()}")
    }

    @After
    fun removeFolderAndKey() {
        folder.deleteRecursively()
        key.forget()
    }

    private val secret get() = ByteArray(32) { (it * 7).toByte() }

    @Test
    fun aSecretSealedByTheKeystoreReadsBack() {
        val store = AndroidDeviceKeyStore(folder, key.sealer())
        store.create("device-key-abc", secret)
        assertArrayEquals(secret, AndroidDeviceKeyStore(folder, key.sealer()).read("device-key-abc"))
    }

    @Test
    fun anExistingSecretIsNeverReplaced() {
        val store = AndroidDeviceKeyStore(folder, key.sealer())
        store.create("device-key-abc", secret)
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store.create("device-key-abc", ByteArray(32)) }
        assertArrayEquals(secret, store.read("device-key-abc"))
    }

    @Test
    fun aSecretRenamedToAnotherAccountWontOpen() {
        val store = AndroidDeviceKeyStore(folder, key.sealer())
        store.create("device-key-abc", secret)
        folder.resolve("device-key-abc.sealed").copyTo(folder.resolve("device-key-def.sealed"))
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store.read("device-key-def") }
    }

    @Test
    fun aDeletedSecretIsGone() {
        val store = AndroidDeviceKeyStore(folder, key.sealer())
        store.create("device-key-abc", secret)
        store.delete("device-key-abc")
        assertNull(store.read("device-key-abc"))
    }
}
