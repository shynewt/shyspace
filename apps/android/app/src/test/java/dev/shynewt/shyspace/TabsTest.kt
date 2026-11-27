package dev.shynewt.shyspace

import androidx.compose.runtime.mutableStateListOf
import org.junit.Assert.assertEquals
import org.junit.Test

class TabsTest {
    private val backStack = mutableStateListOf<Route>(Route.Home)
    private val tabs = Tabs(backStack)

    @Test
    fun backFromTheYouTabReturnsHome() {
        tabs.select(MainTab.YOU)
        assertEquals(listOf(Route.Home, Route.You), backStack.toList())

        tabs.back()

        assertEquals(listOf(Route.Home), backStack.toList())
    }

    @Test
    fun choosingATabLeavesWhatWasOpenBehindIt() {
        backStack.add(Route.Channel("w1", "c1", "#general"))

        tabs.select(MainTab.YOU)
        tabs.select(MainTab.HOME)

        assertEquals(listOf(Route.Home), backStack.toList())
    }
}
