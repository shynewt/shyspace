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

    /** The core's linking, scripted: whole codes are listed, and each step answers when the test completes it. */
    private class ScriptedLinking(private val wholeCodes: Set<String>) : PhoneLinking {
        val calls = mutableListOf<String>()
        val check = CompletableDeferred<LinkCheck>()
        val handover = CompletableDeferred<String>()

        override fun readLinkCode(text: String): String {
            if (text !in wholeCodes) throw ShyException.Failed("This isn't a link code. Copy it again from your other device.")
            return "Acme"
        }

        override suspend fun beginLink(code: String): LinkCheck {
            synchronized(calls) { calls += "begin $code" }
            return check.await()
        }

        override suspend fun waitForLinkHandover(): String = handover.await()

        override suspend fun cancelLink() {
            synchronized(calls) { calls += "cancel" }
        }

        fun callsSoFar() = synchronized(calls) { calls.toList() }
    }

    private val linking = ScriptedLinking(wholeCodes = setOf("shy-link-1234", "shy-link-5678"))
    private val link = mainThread.viewModel { LinkViewModel(linking) }

    private suspend fun stepWhere(predicate: (LinkStep) -> Boolean): LinkStep = withTimeout(5_000) { link.step.first(predicate) }

    @Test
    fun aCodeIsTakenTheMomentItIsWholeWhileAHalfTypedOneStaysQuiet() = runBlocking {
        link.codeChanged("shy-li")
        assertEquals(LinkStep.EnterCode(), link.step.value)

        link.codeChanged("shy-link-1234")

        stepWhere { it == LinkStep.Waiting }
        withTimeout(5_000) { while (linking.callsSoFar().isEmpty()) delay(10) }
        assertEquals(listOf("begin shy-link-1234"), linking.callsSoFar())
    }

    @Test
    fun aSubmittedCodeThatIsNotOneSaysWhyInTheCoresWords() {
        link.codeSubmitted("hello there")

        assertEquals(LinkStep.EnterCode(problem = "This isn't a link code. Copy it again from your other device."), link.step.value)
        assertEquals(emptyList<String>(), linking.callsSoFar())
    }

    @Test
    fun aCodeLeadsToTheNumberThenToReading() = runBlocking {
        link.codeSubmitted("shy-link-1234")

        linking.check.complete(LinkCheck.ShowNumber("482917"))
        stepWhere { it == LinkStep.Checking("482917") }
        linking.handover.complete("w1")

        stepWhere { it == LinkStep.Reading("w1") }
        Unit
    }

    @Test
    fun aRefusedLinkEndsAndCanStartAgain() = runBlocking {
        linking.check.completeExceptionally(ShyException.Failed("Your other device stopped the link. Nothing was shared."))

        link.codeSubmitted("shy-link-1234")
        stepWhere { it == LinkStep.Ended("Your other device stopped the link. Nothing was shared.") }
        link.startAgain()

        assertEquals(LinkStep.EnterCode(), link.step.value)
    }

    @Test
    fun whenSomeoneElseUsedTheCodeItStopsThereAndCanStartAgain() = runBlocking {
        linking.check.complete(LinkCheck.SomeoneElse)

        link.codeSubmitted("shy-link-1234")
        stepWhere { it == LinkStep.SomeoneElse }
        link.startAgain()

        assertEquals(LinkStep.EnterCode(), link.step.value)
    }

    @Test
    fun alreadyConfirmedGoesStraightOnToReading() = runBlocking {
        linking.check.complete(LinkCheck.AlreadyConfirmed)
        linking.handover.complete("w1")

        link.codeSubmitted("shy-link-1234")

        stepWhere { it == LinkStep.Reading("w1") }
        Unit
    }

    @Test
    fun leavingPartWayStopsTheLink() = runBlocking {
        link.codeSubmitted("shy-link-1234")
        withTimeout(5_000) { while (linking.callsSoFar().isEmpty()) delay(10) }

        link.stop()

        withTimeout(5_000) { while ("cancel" !in linking.callsSoFar()) delay(10) }
        assertEquals(listOf("begin shy-link-1234", "cancel"), linking.callsSoFar())
    }

    @Test
    fun aSecondCodeWhileOneIsUnderWayIsIgnored() = runBlocking {
        link.codeSubmitted("shy-link-1234")
        stepWhere { it == LinkStep.Waiting }

        link.codeChanged("shy-link-5678")
        link.codeSubmitted("shy-link-5678")
        linking.check.complete(LinkCheck.ShowNumber("482917"))
        stepWhere { it is LinkStep.Checking }

        assertEquals(listOf("begin shy-link-1234"), linking.callsSoFar())
    }
}
