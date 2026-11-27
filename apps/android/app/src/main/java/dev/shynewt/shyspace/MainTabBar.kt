package dev.shynewt.shyspace

import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import dev.shynewt.shyspace.ui.BadgeTone
import dev.shynewt.shyspace.ui.CountBadge
import dev.shynewt.shyspace.ui.theme.LocalShyColors

enum class MainTab(val label: Int, val icon: Int) {
    HOME(R.string.home, R.drawable.ic_home),
    ACTIVITY(R.string.activity, R.drawable.ic_inbox),
    YOU(R.string.you, R.drawable.ic_person),
}

/** The tabs at the bottom of the top-level screens; Activity shows how many mentions and replies wait. */
@Composable
fun MainTabBar(selected: MainTab, activityCount: Int, onSelect: (MainTab) -> Unit) {
    val colors = LocalShyColors.current
    NavigationBar(containerColor = colors.palette.bg) {
        MainTab.entries.forEach { tab ->
            val waiting = if (tab == MainTab.ACTIVITY && activityCount > 0) {
                pluralStringResource(R.plurals.waiting_activity, activityCount, activityCount)
            } else {
                null
            }
            NavigationBarItem(
                selected = tab == selected,
                onClick = { onSelect(tab) },
                icon = {
                    BadgedBox(
                        // The tab clears its icon's semantics, so the count is read from the tab's state.
                        badge = { if (waiting != null) CountBadge("$activityCount", BadgeTone.DANGER, waiting) },
                    ) { Icon(painterResource(tab.icon), contentDescription = null) }
                },
                label = { Text(stringResource(tab.label)) },
                modifier = Modifier.semantics { waiting?.let { stateDescription = it } },
                colors = NavigationBarItemDefaults.colors(
                    selectedIconColor = colors.accent.accentText,
                    selectedTextColor = colors.palette.text,
                    indicatorColor = colors.accent.accentSoft,
                    unselectedIconColor = colors.palette.textMuted,
                    unselectedTextColor = colors.palette.textMuted,
                ),
            )
        }
    }
}
