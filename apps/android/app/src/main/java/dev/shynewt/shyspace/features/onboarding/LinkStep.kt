package dev.shynewt.shyspace.features.onboarding

/** Where linking this phone to a person's account stands, as the new device sees it. */
sealed interface LinkStep {
    /** Waiting for a link code; [problem] says, in the core's words, why the last one didn't work. */
    data class EnterCode(val problem: String? = null) : LinkStep

    /** The code is in; waiting for the other device to open the request. */
    data object Waiting : LinkStep

    /** The other device must confirm by tapping [checkCode] (6 digits) among decoys. */
    data class Checking(val checkCode: String) : LinkStep

    /** The link ended without this phone being added, for [reason] (the core's words). */
    data class Ended(val reason: String) : LinkStep

    /** Linked for reading: [workspaceId] opens, and this phone can send once the workspace adds it. */
    data class Reading(val workspaceId: String) : LinkStep
}
