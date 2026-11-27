package dev.shynewt.shyspace.features.onboarding

/** Where linking this phone to a person's account stands, as the new device sees it. */
sealed interface LinkStep {
    /** Waiting for a link code; [hasExpired] when the last one had run out. */
    data class EnterCode(val hasExpired: Boolean = false) : LinkStep

    /** The other device must confirm by tapping [checkCode] (6 digits) among decoys. */
    data class Checking(val checkCode: String) : LinkStep

    /** Another device answered this code: the person must refuse it on their other device. */
    data object SomeoneElse : LinkStep

    /** The other device refused or got the number wrong: nothing was shared. */
    data object Stopped : LinkStep
}
