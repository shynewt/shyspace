package dev.shynewt.shyspace.features.onboarding

/**
 * Linking this phone to a person's account, as the core offers it: one link at a time, one call
 * per step. Each call fails with the core's own words for the person (ShyException).
 */
interface PhoneLinking {
    /**
     * Whether [text] is a whole link code that hasn't run out, returning the name of the workspace
     * it links into. Reads nothing but the text, so it's fine on every keystroke.
     */
    fun readLinkCode(text: String): String

    /** Starts linking with the code the person's other device shows, and waits until that device opens the request. */
    suspend fun beginLink(code: String): LinkCheck

    /** Waits until the other device hands this phone what it needs; returns the workspace it can now read. */
    suspend fun waitForLinkHandover(): String

    /** Stops the link for good, a running step too; nothing is kept. */
    suspend fun cancelLink()
}

/** What the other device answered once it opened this phone's request. */
sealed interface LinkCheck {
    /** The person taps [code] on their other device. */
    data class ShowNumber(val code: String) : LinkCheck

    /** The person confirmed before this phone showed the number. */
    data object AlreadyConfirmed : LinkCheck
}
