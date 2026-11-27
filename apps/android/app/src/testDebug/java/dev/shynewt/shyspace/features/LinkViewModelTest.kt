package dev.shynewt.shyspace.features

import dev.shynewt.shyspace.MainThreadRule
import dev.shynewt.shyspace.features.onboarding.LinkCheck
import dev.shynewt.shyspace.features.onboarding.LinkStep
import dev.shynewt.shyspace.features.onboarding.LinkViewModel
import dev.shynewt.shyspace.features.onboarding.PhoneLinking
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import uniffi.shy_ffi.ShyException

class LinkViewModelTest {
    @get:Rule
    val mainThread = MainThreadRule()

    /** The core's linking, scripted: each step answers when the test completes it. */
    private class ScriptedLinking : PhoneLinking {
        val calls = mutableListOf<String>()
        val begun = CompletableDeferred<Unit>()
        val check = CompletableDeferred<LinkCheck>()
        val handover = CompletableDeferred<String>()

        override suspend fun beginLink(code: String) {
            synchronized(calls) { calls += "begin $code" }
            begun.await()
        }

        override suspend fun waitForCheck(): LinkCheck = check.await()

        override suspend fun waitForLinkHandover(): String = handover.await()

        override suspend fun cancelLink() {
            synchronized(calls) { calls += "cancel" }
        }
    }

    private val linking = ScriptedLinking()
    private val link = LinkViewModel(linking)

    private suspend fun stepWhere(predicate: (LinkStep) -> Boolean): LinkStep = withTimeout(5_000) { link.step.first(predicate) }

    @Test
    fun aCodeLeadsToTheNumberThenToReading() = runBlocking {
        link.enterCode("  shy-link-1234 \n")
        stepWhere { it == LinkStep.Waiting }

        linking.begun.complete(Unit)
        linking.check.complete(LinkCheck.ShowNumber("482917"))
        stepWhere { it == LinkStep.Checking("482917") }
        linking.handover.complete("w1")

        stepWhere { it == LinkStep.Reading("w1") }
        assertEquals(listOf("begin shy-link-1234"), linking.calls)
    }

    @Test
    fun aCodeThatDoesNotWorkSaysWhyInTheCoresWords() = runBlocking {
        linking.begun.completeExceptionally(ShyException.Failed("This code has run out. Show a new one on your other device."))

        link.enterCode("shy-link-old")

        stepWhere { it == LinkStep.EnterCode(problem = "This code has run out. Show a new one on your other device.") }
        Unit
    }

    @Test
    fun aRefusedLinkEndsAndCanStartAgain() = runBlocking {
        linking.begun.complete(Unit)
        linking.check.completeExceptionally(ShyException.Failed("Your other device stopped the link. Nothing was shared."))

        link.enterCode("shy-link-1234")
        stepWhere { it == LinkStep.Ended("Your other device stopped the link. Nothing was shared.") }
        link.startAgain()

        assertEquals(LinkStep.EnterCode(), link.step.value)
    }

    @Test
    fun whenSomeoneElseUsedTheCodeItStopsThere() = runBlocking {
        linking.begun.complete(Unit)
        linking.check.complete(LinkCheck.SomeoneElse)

        link.enterCode("shy-link-1234")
        stepWhere { it == LinkStep.SomeoneElse }
        link.startAgain()

        assertEquals(LinkStep.EnterCode(), link.step.value)
    }

    @Test
    fun alreadyConfirmedGoesStraightOnToReading() = runBlocking {
        linking.begun.complete(Unit)
        linking.check.complete(LinkCheck.AlreadyConfirmed)
        linking.handover.complete("w1")

        link.enterCode("shy-link-1234")

        stepWhere { it == LinkStep.Reading("w1") }
        Unit
    }

    @Test
    fun leavingPartWayStopsTheLink() = runBlocking {
        link.enterCode("shy-link-1234")
        stepWhere { it == LinkStep.Waiting }

        link.stop()

        withTimeout(5_000) { while (synchronized(linking.calls) { "cancel" !in linking.calls }) delay(10) }
        assertEquals(listOf("begin shy-link-1234", "cancel"), synchronized(linking.calls) { linking.calls.toList() })
    }

    @Test
    fun aSecondCodeWhileOneIsUnderWayIsIgnored() = runBlocking {
        link.enterCode("shy-link-1234")
        stepWhere { it == LinkStep.Waiting }

        link.enterCode("shy-link-5678")
        linking.begun.complete(Unit)
        linking.check.complete(LinkCheck.ShowNumber("482917"))
        stepWhere { it is LinkStep.Checking }

        assertEquals(listOf("begin shy-link-1234"), synchronized(linking.calls) { linking.calls.toList() })
    }
}
