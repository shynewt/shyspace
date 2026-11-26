package dev.shynewt.shyspace.core

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import uniffi.shy_ffi.InternalException
import uniffi.shy_ffi.ShyException

class CoreCallTest {
    private val failures = mutableListOf<String>()

    @Test
    fun aCallThatWorksGivesItsResult() = runBlocking {
        assertEquals(42, coreCall(onFailure = { failures += it }) { 42 })
        assertEquals(emptyList<String>(), failures)
    }

    @Test
    fun theCoresOwnErrorSaysWhy() = runBlocking {
        assertNull(coreCall(onFailure = { failures += it }) { throw ShyException.Failed("the bucket didn't answer") })
        assertEquals(listOf("the bucket didn't answer"), failures)
    }

    @Test
    fun aCrashInsideTheCoreIsACalmErrorNotACrash() = runBlocking {
        assertNull(coreCall(onFailure = { failures += it }) { throw InternalException("Rust panic") })
        assertEquals(listOf(CORE_CRASHED), failures)
    }
}
