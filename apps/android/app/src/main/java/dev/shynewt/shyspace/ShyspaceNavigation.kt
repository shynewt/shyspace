package dev.shynewt.shyspace

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.saveable.listSaver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import androidx.navigation3.ui.NavDisplay
import dev.shynewt.shyspace.core.ShyCore
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.features.channel.ChannelViewModel
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.HomeViewModel
import dev.shynewt.shyspace.features.home.NoWorkspace
import dev.shynewt.shyspace.features.home.workspaceCreator

sealed interface Route {
    data object Home : Route

    data class Channel(val workspaceId: String, val channelId: String, val title: String) : Route
}

/** The back stack, kept across process death as plain strings. */
private val backStackSaver = listSaver<SnapshotStateList<Route>, List<String>>(
    save = { stack ->
        stack.map { route ->
            when (route) {
                Route.Home -> listOf("home")
                is Route.Channel -> listOf("channel", route.workspaceId, route.channelId, route.title)
            }
        }
    },
    restore = { saved ->
        saved.map { parts ->
            when (parts.first()) {
                "channel" -> Route.Channel(parts[1], parts[2], parts[3])
                else -> Route.Home
            }
        }.toMutableStateList()
    },
)

private fun List<Route>.toMutableStateList() = mutableStateListOf<Route>().also { it.addAll(this) }

@Composable
fun ShyspaceNavigation(core: ShyCore) {
    val backStack = rememberSaveable(saver = backStackSaver) { mutableStateListOf<Route>(Route.Home) }
    NavDisplay(
        backStack = backStack,
        onBack = { backStack.removeLastOrNull() },
        entryDecorators = listOf(
            rememberSaveableStateHolderNavEntryDecorator(),
            rememberViewModelStoreNavEntryDecorator(),
        ),
        entryProvider = entryProvider {
            entry<Route.Home> {
                val home = viewModel { HomeViewModel(core) }
                val state by home.state.collectAsStateWithLifecycle()
                HomeScreen(
                    state = state,
                    onOpenChannel = { channel, title ->
                        (state as? HomeState.Ready)?.let { ready ->
                            backStack.add(Route.Channel(ready.workspaceId, channel.id, title))
                        }
                    },
                    noWorkspaceContent = {
                        NoWorkspace { workspaceCreator?.invoke(home::createWorkspace) }
                    },
                )
            }
            entry<Route.Channel> { route ->
                val channel = viewModel(key = "${route.workspaceId}/${route.channelId}") {
                    ChannelViewModel(core, route.workspaceId, route.channelId)
                }
                val state by channel.state.collectAsStateWithLifecycle()
                ChannelScreen(title = route.title, state = state, onBack = { backStack.removeLastOrNull() }, onSend = channel::send)
            }
        },
    )
}
