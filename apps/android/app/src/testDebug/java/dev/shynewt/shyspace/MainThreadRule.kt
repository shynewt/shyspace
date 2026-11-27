package dev.shynewt.shyspace

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
 * A single thread standing in for the phone's main thread. Resuming inline instead
 * (Dispatchers.Unconfined) would run screen code inside the core's own callbacks. A crash on
 * it fails the test, as it would crash the app.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MainThreadRule : TestWatcher() {
    private val crashes = CopyOnWriteArrayList<Throwable>()
    private val executor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "test-main").apply { setUncaughtExceptionHandler { _, crash -> crashes += crash } }
    }

    override fun starting(description: Description) = Dispatchers.setMain(executor.asCoroutineDispatcher())

    override fun finished(description: Description) {
        Dispatchers.resetMain()
        executor.shutdownNow()
        crashes.firstOrNull()?.let { throw AssertionError("the main thread crashed, as the app would", it) }
    }
}
