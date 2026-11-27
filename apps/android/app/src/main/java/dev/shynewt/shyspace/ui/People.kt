package dev.shynewt.shyspace.ui

/** The name to show for a member: theirs, or "member" and the start of their id until their profile arrives, as on the desktop. */
fun displayName(name: String, id: String): String = name.trim().ifEmpty { "member ${id.take(6)}" }
