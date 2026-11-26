package dev.shynewt.shyspace

import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.shy_ffi.ShyApp

class ReleaseBindingsTest {
    @Test
    fun releaseBuildsCantUseAMemoryBucket() {
        val names = ShyApp.Companion::class.java.methods.map { it.name }
        assertTrue("constructor list: $names", names.none { it.contains("MemoryBucket", ignoreCase = true) })
    }
}
