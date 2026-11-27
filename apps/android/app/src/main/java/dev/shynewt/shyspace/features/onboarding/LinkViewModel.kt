package dev.shynewt.shyspace.features.onboarding

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.coreCall
import uniffi.shy_ffi.ShyException
import dev.shynewt.shyspace.core.reason
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Linking this phone, from the code to reading: each step of [PhoneLinking] in turn. */
class LinkViewModel(private val linking: PhoneLinking) : ViewModel() {
    private val mutableStep = MutableStateFlow<LinkStep>(LinkStep.EnterCode())
    val step: StateFlow<LinkStep> = mutableStep.asStateFlow()

    /** The code field changed: a code is taken as soon as it's whole, typed or pasted. */
    fun codeChanged(text: String) {
        if (mutableStep.value !is LinkStep.EnterCode || !isWholeCode(text)) return
        begin(text)
    }

    /** A code was handed in (pasted, scanned, or typed and Done): taken, or the core says why not. */
    fun codeSubmitted(text: String) {
        if (mutableStep.value !is LinkStep.EnterCode) return
        try {
            linking.readLinkCode(text)
        } catch (problem: ShyException) {
            mutableStep.value = LinkStep.EnterCode(problem = problem.reason())
            return
        }
        begin(text)
    }

    /** Picks up a link a restart interrupted, still waiting for its hand-over: at its number, if it had one. */
    fun resume(checkCode: String?) {
        if (mutableStep.value !is LinkStep.EnterCode) return
        mutableStep.value = checkCode?.let(LinkStep::Checking) ?: LinkStep.Waiting
        viewModelScope.launch { awaitHandover() }
    }

    /** After the link ended: back to entering a code. */
    fun startAgain() {
        if (mutableStep.value !is LinkStep.Ended) return
        mutableStep.value = LinkStep.EnterCode()
    }

    /**
     * The person left the link screen part way: the link stops for good, and nothing is kept. The
     * core stops a running step itself, which then ends here as usual. Only for leaving the
     * screen: closing the app keeps the link, to pick up at the next start.
     */
    suspend fun stop() {
        val step = mutableStep.value
        if (step !is LinkStep.Waiting && step !is LinkStep.Checking) return
        // The screen is closing, so a failure can only go to the log.
        coreCall({ reason -> Log.w(TAG, "couldn't stop linking: $reason") }) { linking.cancelLink() }
    }

    /** Whether [text] is a whole link code that hasn't run out, as the core reads it. */
    fun isWholeCode(text: String): Boolean = try {
        linking.readLinkCode(text)
        true
    } catch (_: ShyException) {
        false
    }

    private fun begin(code: String) {
        mutableStep.value = LinkStep.Waiting
        viewModelScope.launch { link(code) }
    }

    private suspend fun link(code: String) {
        // The code read as whole, so a failure from here on is the link ending.
        val check = coreCall(::end) { linking.beginLink(code) } ?: return
        if (check is LinkCheck.ShowNumber) mutableStep.value = LinkStep.Checking(check.code)
        awaitHandover()
    }

    private suspend fun awaitHandover() {
        val workspaceId = coreCall(::end) { linking.waitForLinkHandover() } ?: return
        mutableStep.value = LinkStep.Reading(workspaceId)
    }

    private fun end(reason: String) {
        mutableStep.value = LinkStep.Ended(reason)
    }

    private companion object {
        const val TAG = "LinkViewModel"
    }
}
