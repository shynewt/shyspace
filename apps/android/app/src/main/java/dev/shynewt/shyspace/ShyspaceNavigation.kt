package dev.shynewt.shyspace

import dev.shynewt.shyspace.ui.LastInput
import dev.shynewt.shyspace.ui.recordsInput
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import uniffi.shy_core.InboxItem
import androidx.compose.ui.res.stringResource
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
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
import dev.shynewt.shyspace.features.activity.ActivityScreen
import dev.shynewt.shyspace.features.activity.ActivityViewModel
import dev.shynewt.shyspace.features.channel.ChannelScreen
import dev.shynewt.shyspace.features.channel.ChannelViewModel
import dev.shynewt.shyspace.features.channel.ThreadScreen
import dev.shynewt.shyspace.features.channel.ThreadViewModel
import dev.shynewt.shyspace.features.home.HomeScreen
import dev.shynewt.shyspace.features.home.HomeState
import dev.shynewt.shyspace.features.home.HomeViewModel
import dev.shynewt.shyspace.features.home.NoWorkspace
import dev.shynewt.shyspace.features.home.workspaceCreator
import dev.shynewt.shyspace.features.settings.AppSettings
import dev.shynewt.shyspace.features.settings.YouScreen
import androidx.navigation3.ui.NavDisplay.popTransitionSpec
import androidx.navigation3.ui.NavDisplay.predictivePopTransitionSpec
import androidx.navigation3.ui.NavDisplay.transitionSpec

sealed interface Route {
    data object Home : Route

    data class Channel(val workspaceId: String, val channelId: String, val title: String) : Route

    data class Thread(val workspaceId: String, val channelId: String, val rootId: String, val channelTitle: String) : Route

    /** The You tab; it sits on Home, so Back returns there, as Android's tabs do. */
    data object Activity : Route

    data object You : Route
}

/** The back stack, kept across process death as plain strings. */
private val backStackSaver = listSaver<SnapshotStateList<Route>, List<String>>(
    save = { stack ->
        stack.map { route ->
            when (route) {
                Route.Home -> listOf("home")
                is Route.Channel -> listOf("channel", route.workspaceId, route.channelId, route.title)
                is Route.Thread -> listOf("thread", route.workspaceId, route.channelId, route.rootId, route.channelTitle)
                Route.Activity -> listOf("activity")
                Route.You -> listOf("you")
            }
        }
    },
    restore = { saved ->
        saved.map { parts ->
            when (parts.first()) {
                "channel" -> Route.Channel(parts[1], parts[2], parts[3])
                "thread" -> Route.Thread(parts[1], parts[2], parts[3], parts[4])
                "activity" -> Route.Activity
                "you" -> Route.You
                else -> Route.Home
            }
        }.toMutableStateList()
    },
)

private fun List<Route>.toMutableStateList() = mutableStateListOf<Route>().also { it.addAll(this) }

@Composable
fun ShyspaceNavigation(core: ShyCore, settings: AppSettings) {
    val lastInput = remember { LastInput() }
    val backStack = rememberSaveable(saver = backStackSaver) { mutableStateListOf<Route>(Route.Home) }
    val tabs = Tabs(backStack)
    // Kept for the whole app, so Activity follows the workspace Home shows, on any tab.
    val home = viewModel { HomeViewModel(core) }
    val homeState by home.state.collectAsStateWithLifecycle()
    val ready = homeState as? HomeState.Ready
    val activity = viewModel { ActivityViewModel(core) }
    val activityState by activity.state.collectAsStateWithLifecycle()
    val noWorkspace = stringResource(R.string.no_workspace)
    LaunchedEffect(homeState) {
        when (val state = homeState) {
            is HomeState.Ready -> activity.follow(state.workspaceId)
            is HomeState.NoWorkspace -> activity.showUnavailable(noWorkspace)
            is HomeState.Failed -> activity.showUnavailable(state.reason)
            HomeState.Loading -> Unit
        }
    }
    val tabBar: @Composable (MainTab) -> Unit = { selected ->
        MainTabBar(selected, activityCount = activityState.attention.size, onSelect = { tabs.select(it) })
    }
    Box(modifier = Modifier.fillMaxSize().recordsInput(lastInput)) {
        NavDisplay(
            backStack = backStack,
            onBack = tabs::back,
            transitionSpec = { openMotion(lastInput) },
            popTransitionSpec = { closeMotion(lastInput) },
            predictivePopTransitionSpec = { predictiveBackMotion() },
            entryDecorators = listOf(
                rememberSaveableStateHolderNavEntryDecorator(),
                rememberViewModelStoreNavEntryDecorator(),
            ),
            entryProvider = entryProvider {
                entry<Route.Home> {
                    HomeScreen(
                        state = homeState,
                        onOpenChannel = { channel, title ->
                            ready?.let { backStack.add(Route.Channel(it.workspaceId, channel.id, title)) }
                        },
                        onRetry = home::retry,
                        bottomBar = { tabBar(MainTab.HOME) },
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
                        onEdit = channel::edit,
                        onReact = channel::react,
                        onDelete = channel::delete,
                        onLoadOlder = channel::loadOlder,
                        onOpenThread = { root -> backStack.add(Route.Thread(route.workspaceId, route.channelId, root.id, route.title)) },
                    )
                }
                entry<Route.Thread> { route ->
                    val thread = viewModel(key = "${route.workspaceId}/${route.rootId}") {
                        ThreadViewModel(core, route.workspaceId, route.channelId, route.rootId)
                    }
                    val state by thread.state.collectAsStateWithLifecycle()
                    ThreadScreen(
                        channelTitle = route.channelTitle,
                        state = state,
                        onBack = tabs::back,
                        onReply = thread::reply,
                        onSeen = thread::markSeen,
                        onEdit = thread::edit,
                        onReact = thread::react,
                        onDelete = thread::delete,
                    )
                }
                entry<Route.Activity>(metadata = switchesAtOnce) {
                    ActivityScreen(
                        state = activityState,
                        channels = ready?.channels.orEmpty(),
                        members = ready?.members.orEmpty(),
                        onOpenItem = { item, title ->
                            activityState.workspaceId?.let { backStack.add(routeTo(item, it, title)) }
                        },
                        onOpenChannel = { channel, title ->
                            activityState.workspaceId?.let { backStack.add(Route.Channel(it, channel.id, title)) }
                        },
                        onMarkRead = activity::markChannelRead,
                        bottomBar = { tabBar(MainTab.ACTIVITY) },
                    )
                }
                entry<Route.You>(metadata = switchesAtOnce) {
                    val hidesScreenContent by settings.hidesScreenContent.collectAsStateWithLifecycle()
                    val saveFailed by settings.saveFailed.collectAsStateWithLifecycle()
                    YouScreen(
                        hidesScreenContent = hidesScreenContent,
                        onHidesScreenContentChange = settings::setHidesScreenContent,
                        saveFailed = saveFailed,
                        bottomBar = { tabBar(MainTab.YOU) },
                    )
                }
            },
        )
    }
}

/** Where an Activity item opens: its thread when it's in one, mentions in a thread too. */
internal fun routeTo(item: InboxItem, workspaceId: String, channelTitle: String): Route {
    val root = item.threadRoot ?: return Route.Channel(workspaceId, item.message.channel, channelTitle)
    return Route.Thread(workspaceId, item.message.channel, root, channelTitle)
}

/** Switching tabs replaces the screen at once; only going deeper moves (see NavMotion.kt). */
private val switchesAtOnce =
    transitionSpec { atOnce() } + popTransitionSpec { atOnce() } + predictivePopTransitionSpec { atOnce() }

/** Moving between screens: the tabs, and Back. */
internal class Tabs(private val backStack: SnapshotStateList<Route>) {
    /** Activity and You sit on Home, so Back from them returns Home, as Android's tabs do. */
    fun select(tab: MainTab) {
        backStack.clear()
        backStack.add(Route.Home)
        when (tab) {
            MainTab.HOME -> Unit
            MainTab.ACTIVITY -> backStack.add(Route.Activity)
            MainTab.YOU -> backStack.add(Route.You)
        }
    }

    fun back() {
        backStack.removeLastOrNull()
    }
}
