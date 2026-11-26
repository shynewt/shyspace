package dev.shynewt.shyspace.core

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.nio.channels.FileChannel
import java.nio.file.Files
import java.nio.file.NoSuchFileException
import java.nio.file.StandardOpenOption
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.shy_ffi.DeviceKeyStore
import uniffi.shy_ffi.DeviceKeyStoreException

/** Seals secrets so only this app on this phone can open them, each bound to its account. */
interface SecretSealer {
    /** Whether the key that seals and opens exists. */
    fun hasKey(): Boolean

    /** Makes that key; fails if it exists. */
    fun createKey()

    fun seal(account: String, secret: ByteArray): ByteArray

    fun open(account: String, sealed: ByteArray): ByteArray
}

/**
 * The core's device keys, one sealed file per account in [folder]. Keeps the core's contract:
 * `read` answers null only when there's no file, `create` never replaces a secret and returns
 * only once it's on disk, and no secret appears in an error.
 */
class AndroidDeviceKeyStore(private val folder: File, private val sealer: SecretSealer) : DeviceKeyStore {
    override fun read(account: String): ByteArray? {
        val sealed = try {
            Files.readAllBytes(fileFor(account).toPath())
        } catch (error: NoSuchFileException) {
            return null
        } catch (error: Exception) {
            throw failed("couldn't read the device key for $account", error)
        }
        return try {
            sealer.open(account, sealed)
        } catch (error: Exception) {
            throw failed("couldn't open the device key for $account", error)
        }
    }

    override fun create(account: String, secret: ByteArray) {
        val target = fileFor(account)
        try {
            folder.mkdirs()
            val pending = File.createTempFile("pending-", ".tmp", folder)
            try {
                writeDurably(pending, sealWithKey(account, secret))
                synchronized(writeLock) {
                    if (target.exists()) throw failed("$account already has a device key", null)
                    if (!pending.renameTo(target)) throw failed("couldn't save the device key for $account", null)
                    syncFolder()
                }
            } finally {
                pending.delete()
            }
        } catch (error: DeviceKeyStoreException) {
            throw error
        } catch (error: Exception) {
            throw failed("couldn't save the device key for $account", error)
        } finally {
            secret.fill(0)
        }
    }

    override fun delete(account: String) {
        val file = fileFor(account)
        try {
            synchronized(writeLock) {
                if (Files.deleteIfExists(file.toPath())) syncFolder()
            }
        } catch (error: Exception) {
            throw failed("couldn't delete the device key for $account", error)
        }
    }

    /**
     * Makes the sealing key the first time only. If sealed keys exist but their sealing key is
     * gone (the app's Keystore entries were cleared), a new one would just hide that, so it's an
     * error instead.
     */
    private fun sealWithKey(account: String, secret: ByteArray): ByteArray {
        synchronized(writeLock) {
            if (!sealer.hasKey()) {
                // A listing that fails throws, rather than looking empty and making a new key.
                val hasSealedKeys = Files.newDirectoryStream(folder.toPath(), "*$SEALED").use { it.iterator().hasNext() }
                if (hasSealedKeys) throw failed("this phone's device keys can no longer be opened", null)
                sealer.createKey()
            }
        }
        return sealer.seal(account, secret)
    }

    private fun writeDurably(file: File, bytes: ByteArray) {
        FileChannel.open(file.toPath(), StandardOpenOption.WRITE, StandardOpenOption.TRUNCATE_EXISTING).use { channel ->
            channel.write(java.nio.ByteBuffer.wrap(bytes))
            channel.force(true)
        }
    }

    /** Makes a rename or delete in the folder survive a power cut. */
    private fun syncFolder() {
        FileChannel.open(folder.toPath(), StandardOpenOption.READ).use { it.force(true) }
    }

    private fun fileFor(account: String): File {
        if (!PLAIN_NAME.matches(account)) throw failed("$account isn't a key name", null)
        return folder.resolve("$account$SEALED")
    }

    private fun failed(reason: String, cause: Exception?) =
        DeviceKeyStoreException.Failed(if (cause == null) reason else "$reason: ${cause.javaClass.simpleName}")

    private companion object {
        const val SEALED = ".sealed"
        val PLAIN_NAME = Regex("[A-Za-z0-9_-]{1,128}")

        /** One writer at a time, across every instance in the process. */
        val writeLock = Any()
    }
}

/**
 * AES-256-GCM with a key that never leaves the Android Keystore. The account is the associated
 * data, so a sealed file renamed to another account won't open.
 */
class KeystoreSealer(private val alias: String = ALIAS) : SecretSealer {
    override fun hasKey(): Boolean = existingKey() != null

    override fun createKey() {
        check(existingKey() == null) { "the sealing key exists" }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        generator.generateKey()
    }

    override fun seal(account: String, secret: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, requiredKey())
        cipher.updateAAD(account.toByteArray())
        return cipher.iv + cipher.doFinal(secret)
    }

    override fun open(account: String, sealed: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, requiredKey(), GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES))
        cipher.updateAAD(account.toByteArray())
        return cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES)
    }

    private fun existingKey(): SecretKey? =
        KeyStore.getInstance(KEYSTORE).apply { load(null) }.getKey(alias, null) as SecretKey?

    private fun requiredKey(): SecretKey = checkNotNull(existingKey()) { "the sealing key is missing" }

    private companion object {
        const val KEYSTORE = "AndroidKeyStore"
        const val ALIAS = "shyspace-device-keys"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val TAG_BITS = 128
    }
}
