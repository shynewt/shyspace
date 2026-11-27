package dev.shynewt.shyspace

import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.ViewModelStore
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModel
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.setMain
import org.junit.rules.TestWatcher
import org.junit.runner.Description

/**
 * A single thread standing in for the phone's main thread, and the screens' lifetime: view
 * models made through [viewModel] are cleared when the test ends, as when a screen goes, so no
 * work outlives its test. Resuming inline instead (Dispatchers.Unconfined) would run screen
 * code inside the core's own callbacks. A crash on it fails the test, as it would crash the app.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MainThreadRule : TestWatcher() {
    private val crashes = CopyOnWriteArrayList<Throwable>()
    private val executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "test-main").apply { setUncaughtExceptionHandler { _, crash -> crashes += crash } }
    }

    override fun starting(description: Description) = Dispatchers.setMain(executor.asCoroutineDispatcher())

    @PublishedApi
    internal val viewModels = ViewModelStore()

    @PublishedApi
    internal var viewModelCount = 0

    /** A view model that lives until the test ends. */
    inline fun <reified T : ViewModel> viewModel(crossinline make: () -> T): T =
        ViewModelProvider.create(viewModels, viewModelFactory { initializer { make() } })["test-${viewModelCount++}", T::class]

    override fun finished(description: Description) {
        viewModels.clear()
        Dispatchers.resetMain()
        executor.shutdownNow()
        crashes.firstOrNull()?.let { throw AssertionError("the main thread crashed, as the app would", it) }
        CoreCallbackCrashes.failIfAny()
    }
}
