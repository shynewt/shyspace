package dev.shynewt.shyspace.features.onboarding

import android.util.Log
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dev.shynewt.shyspace.core.coreCall
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** Linking this phone, from the code to reading: each step of [PhoneLinking] in turn. */
class LinkViewModel(private val linking: PhoneLinking) : ViewModel() {
    private val mutableStep = MutableStateFlow<LinkStep>(LinkStep.EnterCode())
    val step: StateFlow<LinkStep> = mutableStep.asStateFlow()
    private var linkingJob: Job? = null

    /** A whole code arrived (pasted, scanned, or typed and confirmed): link with it. */
    fun enterCode(code: String) {
        val trimmed = code.trim()
        if (trimmed.isEmpty() || mutableStep.value !is LinkStep.EnterCode) return
        mutableStep.value = LinkStep.Waiting
        linkingJob = viewModelScope.launch { link(trimmed) }
    }

    /** After the link ended, or someone else used the code: back to entering a code. */
    fun startAgain() {
        val step = mutableStep.value
        if (step !is LinkStep.Ended && step != LinkStep.SomeoneElse) return
        mutableStep.value = LinkStep.EnterCode()
    }

    /** Leaving the screen part way: the link stops, and nothing is kept. */
    fun stop() {
        val isUnderWay = linkingJob?.isActive == true
        linkingJob?.cancel()
        if (!isUnderWay) return
        // The person is leaving the screen, so a failure can only go to the log.
        viewModelScope.launch { coreCall({ reason -> Log.w(TAG, "couldn't stop linking: $reason") }) { linking.cancelLink() } }
    }

    private suspend fun link(code: String) {
        coreCall({ reason -> mutableStep.value = LinkStep.EnterCode(problem = reason) }) { linking.beginLink(code) } ?: return
        val check = coreCall(::end) { linking.waitForCheck() } ?: return
        when (check) {
            is LinkCheck.ShowNumber -> mutableStep.value = LinkStep.Checking(check.code)
            LinkCheck.SomeoneElse -> {
                mutableStep.value = LinkStep.SomeoneElse
                return
            }
            LinkCheck.AlreadyConfirmed -> Unit
        }
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
