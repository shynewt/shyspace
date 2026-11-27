package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.features.onboarding.LinkCheck
import dev.shynewt.shyspace.features.onboarding.LinkKeeper
import dev.shynewt.shyspace.features.onboarding.PhoneLinking
import dev.shynewt.shyspace.features.onboarding.PickedUpLink
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.shy_ffi.ShyException

class LinkKeeperTest {
    /** The core's linking after the hand-over, scripted. */
    private class ScriptedLinking(private val resumed: PickedUpLink?) : PhoneLinking {
        val added = CompletableDeferred<Unit>()
        @Volatile var waits = 0

        override fun readLinkCode(text: String): String = error("not part of waiting")

        override suspend fun beginLink(code: String): LinkCheck = error("not part of waiting")

        override suspend fun waitForLinkHandover(): String = error("not part of waiting")

        override suspend fun waitUntilLinked() {
            waits++
            added.await()
        }

        override suspend fun cancelLink() = error("not part of waiting")

        override suspend fun resumeLink(): PickedUpLink? = resumed
    }

    private val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @After
    fun endTheApp() = appScope.cancel()

    @Test
    fun theWaitToBeAddedRunsOnceHoweverOftenItIsAskedFor() = runBlocking {
        val linking = ScriptedLinking(resumed = null)
        val keeper = LinkKeeper(linking, appScope)

        keeper.waitUntilAdded()
        keeper.waitUntilAdded()
        withTimeout(5_000) { while (linking.waits == 0) delay(10) }
        linking.added.complete(Unit)

        assertEquals(1, linking.waits)
    }

    @Test
    fun aWaitThatEndsWithoutBeingAddedSaysWhy() = runBlocking {
        val linking = ScriptedLinking(resumed = null)
        val keeper = LinkKeeper(linking, appScope)
        linking.added.completeExceptionally(ShyException.Failed("This device wasn't added within a week, so nothing was kept. Link again from your other device."))

        keeper.waitUntilAdded()

        val why = withTimeout(5_000) { keeper.problem.first { it != null } }
        assertEquals("This device wasn't added within a week, so nothing was kept. Link again from your other device.", why)
    }

    @Test
    fun aResumedLinkThatReadsGoesOnWaitingToBeAdded() = runBlocking {
        val linking = ScriptedLinking(resumed = PickedUpLink.Reading("w1"))
        val keeper = LinkKeeper(linking, appScope)

        assertEquals(PickedUpLink.Reading("w1"), keeper.resume())

        withTimeout(5_000) { while (linking.waits == 0) delay(10) }
        assertEquals(1, linking.waits)
    }

    @Test
    fun aResumedLinkStillWaitingForItsHandoverIsForTheLinkScreen() = runBlocking {
        val linking = ScriptedLinking(resumed = PickedUpLink.Waiting(checkCode = "482917"))
        val keeper = LinkKeeper(linking, appScope)

        assertEquals(PickedUpLink.Waiting("482917"), keeper.resume())
        delay(100)

        assertEquals(0, linking.waits)
    }
}
