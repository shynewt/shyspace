package dev.shynewt.shyspace.core

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.security.KeyStore
import javax.net.ssl.KeyManagerFactory
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLServerSocket
import javax.net.ssl.SSLSocket
import kotlin.concurrent.thread
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.shy_core.Networking
import uniffi.shy_ffi.BucketSettings
import uniffi.shy_ffi.ShyApp
import uniffi.shy_ffi.ShyException

/** The core's HTTPS on a real phone. Needs the internet: it talks to Amazon S3. */
@RunWith(AndroidJUnit4::class)
class PlatformTlsDeviceTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext
    private lateinit var folder: File
    private val key = TestSealerKey()

    @Before
    fun useThePhonesTrustStore() {
        PlatformTls.usePhoneTrustStore(context)
        folder = context.noBackupFilesDir.resolve("tls-test-${System.nanoTime()}")
    }

    @After
    fun removeFolderAndKey() {
        folder.deleteRecursively()
        key.forget()
    }

    /** Creating a workspace first checks the bucket, which is the HTTPS request under test. */
    private fun creatingAWorkspaceFails(endpoint: String): String {
        val app = ShyApp(
            folder.resolve("workspaces").path,
            AndroidDeviceKeyStore(folder.resolve("device-keys"), key.sealer()),
            Networking.OFFLINE,
        )
        val unknownKeys = BucketSettings(endpoint, "us-east-1", "shyspace-tls-check", "AKIAEXAMPLEEXAMPLE00", "not-a-real-secret")
        return assertThrows(ShyException::class.java) {
            runBlocking { app.createWorkspace("Acme", "Ada", unknownKeys) }
        }.reason()
    }

    @Test
    fun aBucketWithAValidCertificateIsReached() {
        val reason = creatingAWorkspaceFails("https://s3.us-east-1.amazonaws.com")
        // S3 answering at all means the certificate was checked and accepted.
        assertTrue(reason, reason.contains("403 Forbidden"))
    }

    /**
     * A server on the phone itself, with a certificate no authority signed. Takes about half a
     * minute: the bucket client retries a failed connection ten times.
     */
    @Test
    fun aHostWithASelfSignedCertificateIsRefused() {
        selfSignedServer().use { server ->
            val reason = creatingAWorkspaceFails("https://127.0.0.1:${server.localPort}")
            // Refused for the right reason: no authority the phone trusts signed it.
            assertTrue(reason, reason.contains("invalid peer certificate: UnknownIssuer"))
        }
    }

    /** Accepts connections and offers the self-signed certificate in the test's assets. */
    private fun selfSignedServer(): SSLServerSocket {
        val keys = KeyStore.getInstance("PKCS12")
        InstrumentationRegistry.getInstrumentation().context.assets.open("self-signed.p12").use {
            keys.load(it, PASSWORD)
        }
        val keyManagers = KeyManagerFactory.getInstance(KeyManagerFactory.getDefaultAlgorithm()).apply { init(keys, PASSWORD) }
        val tls = SSLContext.getInstance("TLS").apply { init(keyManagers.keyManagers, null, null) }
        val server = tls.serverSocketFactory.createServerSocket(0, 50, InetAddress.getByName("127.0.0.1")) as SSLServerSocket
        thread(isDaemon = true) {
            while (!server.isClosed) {
                try {
                    server.accept().use { (it as SSLSocket).startHandshake() }
                } catch (error: IOException) {
                    // The client refusing the certificate ends each handshake; keep serving.
                }
            }
        }
        return server
    }

    private companion object {
        val PASSWORD = "test-only".toCharArray()
    }
}
