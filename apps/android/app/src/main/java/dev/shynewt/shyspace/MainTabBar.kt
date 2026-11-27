package dev.shynewt.shyspace

import androidx.compose.material3.Icon
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import dev.shynewt.shyspace.ui.theme.LocalShyColors

enum class MainTab(val label: Int, val icon: Int) {
    HOME(R.string.home, R.drawable.ic_home),
    YOU(R.string.you, R.drawable.ic_person),
}

/** The tabs at the bottom of the top-level screens. */
@Composable
fun MainTabBar(selected: MainTab, onSelect: (MainTab) -> Unit) {
    val colors = LocalShyColors.current
    NavigationBar(containerColor = colors.palette.bg) {
        MainTab.entries.forEach { tab ->
            NavigationBarItem(
                selected = tab == selected,
                onClick = { onSelect(tab) },
                icon = { Icon(painterResource(tab.icon), contentDescription = null) },
                label = { Text(stringResource(tab.label)) },
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
