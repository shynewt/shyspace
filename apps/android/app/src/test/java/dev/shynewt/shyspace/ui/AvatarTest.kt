package dev.shynewt.shyspace.ui

import org.junit.Assert.assertEquals
import org.junit.Test

class AvatarTest {
    @Test
    fun aPersonHasTheSameColourAsOnTheDesktop() {
        // Hues from the desktop's avatarHue (apps/desktop/src/ui/Avatar.tsx) for the same ids.
        val desktop = mapOf(
            "ada" to 288,
            "grace" to 262,
            "linus" to 176,
            "9f86d081884c7d659a2feaa0c55ad015" to 262,
            "ünïcode-名前" to 32,
        )
        desktop.forEach { (id, hue) -> assertEquals(id, hue, avatarHue(id)) }
    }

    @Test
    fun initialsAreTheFirstLettersOfTheFirstTwoWords() {
        assertEquals("AL", initials("Ada Lovelace"))
        assertEquals("G", initials("  grace "))
        assertEquals("GM", initials("Grace Murray Hopper"))
        assertEquals("?", initials(""))
    }
}
