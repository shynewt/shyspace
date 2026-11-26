package dev.shynewt.shyspace.core

import uniffi.shy_ffi.InternalException
import uniffi.shy_ffi.ShyException

/** What failed and why, in words fit to show. */
fun ShyException.reason(): String = when (this) {
    is ShyException.Failed -> reason
}

/** Shown for a crash inside the core, whose own text ("Rust panic") means nothing to people. */
const val CORE_CRASHED = "Something went wrong inside shyspace. Try again, and if it keeps happening, restart the app."

/**
 * Runs [call] on the core, handing a failure to [onFailure] in words fit to show and returning
 * null. A crash inside the core counts as a failure too, so it never brings the app down.
 */
suspend fun <T> coreCall(onFailure: (String) -> Unit, call: suspend () -> T): T? = try {
    call()
} catch (error: ShyException) {
    onFailure(error.reason())
    null
} catch (error: InternalException) {
    onFailure(CORE_CRASHED)
    null
}
