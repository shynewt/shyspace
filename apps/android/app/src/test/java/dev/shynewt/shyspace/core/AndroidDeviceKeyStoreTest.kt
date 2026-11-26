package dev.shynewt.shyspace.core

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.shy_ffi.DeviceKeyStoreException

class AndroidDeviceKeyStoreTest {
    @get:Rule
    val folder = TemporaryFolder()

    /** Seals by tagging with the account, so a file moved to another account doesn't open. */
    private class TaggingSealer(var hasKey: Boolean = false) : SecretSealer {
        var keysMade = 0

        override fun hasKey() = hasKey

        override fun createKey() {
            check(!hasKey)
            hasKey = true
            keysMade++
        }

        override fun seal(account: String, secret: ByteArray): ByteArray {
            check(hasKey)
            return account.toByteArray() + 0 + secret
        }

        override fun open(account: String, sealed: ByteArray): ByteArray {
            check(hasKey)
            val tag = account.toByteArray() + 0
            if (!sealed.take(tag.size).toByteArray().contentEquals(tag)) error("sealed for another account")
            return sealed.drop(tag.size).toByteArray()
        }
    }

    private val sealer = TaggingSealer()

    private fun store() = AndroidDeviceKeyStore(folder.root, sealer)

    private val secret get() = ByteArray(32) { it.toByte() }

    @Test
    fun anAccountWithoutASecretReadsAsNone() {
        assertNull(store().read("device-key-abc"))
    }

    @Test
    fun aCreatedSecretReadsBack() {
        val store = store()
        store.create("device-key-abc", secret)
        assertArrayEquals(secret, store.read("device-key-abc"))
        assertArrayEquals(secret, store().read("device-key-abc"))
    }

    @Test
    fun anExistingSecretIsNeverReplaced() {
        val store = store()
        store.create("device-key-abc", secret)
        assertThrows(DeviceKeyStoreException.Failed::class.java) {
            store.create("device-key-abc", ByteArray(32) { 7 })
        }
        assertArrayEquals(secret, store.read("device-key-abc"))
    }

    @Test
    fun aSecretThatWontOpenIsAnErrorNotNone() {
        val store = store()
        store.create("device-key-abc", secret)
        folder.root.resolve("device-key-abc.sealed").copyTo(folder.root.resolve("device-key-def.sealed"))
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store.read("device-key-def") }
    }

    @Test
    fun deletingIsDoneOnceAndFineTwice() {
        val store = store()
        store.create("device-key-abc", secret)
        store.delete("device-key-abc")
        assertNull(store.read("device-key-abc"))
        store.delete("device-key-abc")
    }

    @Test
    fun noHalfWrittenFileIsLeftBehind() {
        store().create("device-key-abc", secret)
        assertEquals(listOf("device-key-abc.sealed"), folder.root.list()!!.toList())
    }

    @Test
    fun theSealingKeyIsMadeOnceForTheFirstSecret() {
        val store = store()
        store.create("device-key-abc", secret)
        store.create("device-key-def", secret)
        assertEquals(1, sealer.keysMade)
    }

    @Test
    fun aLostSealingKeyIsAnErrorNotAFreshStart() {
        val store = store()
        store.create("device-key-abc", secret)
        sealer.hasKey = false
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store.create("device-key-def", secret) }
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store.read("device-key-abc") }
        assertEquals(1, sealer.keysMade)
    }

    @Test
    fun theSecretIsClearedFromMemoryAfterSaving() {
        val given = secret
        store().create("device-key-abc", given)
        assertTrue(given.all { it == 0.toByte() })
    }

    @Test
    fun anAccountThatIsntAPlainNameIsRefused() {
        assertThrows(DeviceKeyStoreException.Failed::class.java) { store().read("../escape") }
        assertTrue(folder.root.list()!!.isEmpty())
    }
}
