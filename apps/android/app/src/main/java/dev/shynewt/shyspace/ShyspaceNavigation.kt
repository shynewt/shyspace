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
import dev.shynewt.shyspace.features.settings.AppSettings
import dev.shynewt.shyspace.features.settings.YouScreen
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.togetherWith
import androidx.navigation3.ui.NavDisplay.popTransitionSpec
import androidx.navigation3.ui.NavDisplay.predictivePopTransitionSpec
import androidx.navigation3.ui.NavDisplay.transitionSpec

sealed interface Route {
    data object Home : Route

    data class Channel(val workspaceId: String, val channelId: String, val title: String) : Route

    /** The You tab; it sits on Home, so Back returns there, as Android's tabs do. */
    data object You : Route
}

/** The back stack, kept across process death as plain strings. */
private val backStackSaver = listSaver<SnapshotStateList<Route>, List<String>>(
    save = { stack ->
        stack.map { route ->
            when (route) {
                Route.Home -> listOf("home")
                is Route.Channel -> listOf("channel", route.workspaceId, route.channelId, route.title)
                Route.You -> listOf("you")
            }
        }
    },
    restore = { saved ->
        saved.map { parts ->
            when (parts.first()) {
                "channel" -> Route.Channel(parts[1], parts[2], parts[3])
                "you" -> Route.You
                else -> Route.Home
            }
        }.toMutableStateList()
    },
)

private fun List<Route>.toMutableStateList() = mutableStateListOf<Route>().also { it.addAll(this) }

@Composable
fun ShyspaceNavigation(core: ShyCore, settings: AppSettings) {
    val backStack = rememberSaveable(saver = backStackSaver) { mutableStateListOf<Route>(Route.Home) }
    val tabs = Tabs(backStack)
    NavDisplay(
        backStack = backStack,
        onBack = tabs::back,
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
                    onRetry = home::retry,
                    bottomBar = { MainTabBar(MainTab.HOME, onSelect = { tabs.select(it) }) },
                    noWorkspaceContent = { noWorkspace ->
                        NoWorkspace(noWorkspace.createError) {
                            workspaceCreator?.invoke(noWorkspace.isCreating, home::createWorkspace)
                        }
                    },
                )
            }
            entry<Route.Channel> { route ->
                val channel = viewModel(key = "${route.workspaceId}/${route.channelId}") {
                    ChannelViewModel(core, route.workspaceId, route.channelId)
                }
                val state by channel.state.collectAsStateWithLifecycle()
                ChannelScreen(
                    title = route.title,
                    state = state,
                    onBack = tabs::back,
                    onSend = channel::send,
                    onSeen = channel::markSeen,
                )
            }
            entry<Route.You>(metadata = switchesAtOnce) {
                val hidesScreenContent by settings.hidesScreenContent.collectAsStateWithLifecycle()
                val saveFailed by settings.saveFailed.collectAsStateWithLifecycle()
                YouScreen(
                    hidesScreenContent = hidesScreenContent,
                    onHidesScreenContentChange = settings::setHidesScreenContent,
                    saveFailed = saveFailed,
                    bottomBar = { MainTabBar(MainTab.YOU, onSelect = { tabs.select(it) }) },
                )
            }
        },
    )
}

/** Switching tabs replaces the screen at once; only going deeper slides. */
private val switchesAtOnce =
    transitionSpec { EnterTransition.None togetherWith ExitTransition.None } +
        popTransitionSpec { EnterTransition.None togetherWith ExitTransition.None } +
        predictivePopTransitionSpec { EnterTransition.None togetherWith ExitTransition.None }

/** Moving between screens: the tabs, and Back. */
internal class Tabs(private val backStack: SnapshotStateList<Route>) {
    /** You sits on Home, so Back from it returns Home, as Android's tabs do. */
    fun select(tab: MainTab) {
        backStack.clear()
        backStack.add(Route.Home)
        if (tab == MainTab.YOU) backStack.add(Route.You)
    }

    fun back() {
        backStack.removeLastOrNull()
    }
}
