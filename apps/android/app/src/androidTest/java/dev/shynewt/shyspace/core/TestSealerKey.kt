package dev.shynewt.shyspace.core

import java.security.KeyStore

/**
 * A sealer under a key of the tests' own, so they never create or depend on the app's real
 * sealing key; [forget] removes it after.
 */
class TestSealerKey {
    val alias = "shyspace-device-keys-test-${System.nanoTime()}"

    fun sealer() = KeystoreSealer(alias)

    fun forget() {
        KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(alias)
    }
}
