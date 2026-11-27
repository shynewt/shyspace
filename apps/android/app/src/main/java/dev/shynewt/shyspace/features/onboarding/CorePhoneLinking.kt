package dev.shynewt.shyspace.features.onboarding

import uniffi.shy_core.CheckOnNewDevice
import uniffi.shy_core.Platform
import uniffi.shy_ffi.ResumedLink
import uniffi.shy_ffi.ShyApp

/** [PhoneLinking] through the core's link calls. */
class CorePhoneLinking(private val app: ShyApp) : PhoneLinking {
    override fun readLinkCode(text: String): String = app.readLinkCode(text).workspaceName

    override suspend fun beginLink(code: String): LinkCheck = when (val check = app.beginLink(code, Platform.ANDROID)) {
        is CheckOnNewDevice.Code -> LinkCheck.ShowNumber(check.v1)
        CheckOnNewDevice.Confirmed -> LinkCheck.AlreadyConfirmed
        // The core ends such a link with its own words for the person instead.
        CheckOnNewDevice.SomeoneElse -> error("the core answered a link someone else used as a check")
    }

    override suspend fun waitForLinkHandover(): String = app.waitForLinkHandover().use { reading -> reading.workspaceId() }

    override suspend fun waitUntilLinked() {
        app.waitUntilLinked().close()
    }

    override suspend fun cancelLink() = app.cancelLink()

    override suspend fun resumeLink(): PickedUpLink? = when (val resumed = app.resumeLink()) {
        null -> null
        is ResumedLink.Waiting -> PickedUpLink.Waiting(resumed.code)
        is ResumedLink.Reading -> PickedUpLink.Reading(resumed.workspace.use { it.workspaceId() })
    }
}
