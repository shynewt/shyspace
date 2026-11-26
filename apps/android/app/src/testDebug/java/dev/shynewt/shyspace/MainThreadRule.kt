package dev.shynewt.shyspace

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
 * (Dispatchers.Unconfined) would run screen code inside the core's own callbacks.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class MainThreadRule : TestWatcher() {
    private val executor = Executors.newSingleThreadExecutor { runnable -> Thread(runnable, "test-main") }

    override fun starting(description: Description) = Dispatchers.setMain(executor.asCoroutineDispatcher())

    override fun finished(description: Description) {
        Dispatchers.resetMain()
        executor.shutdownNow()
    }
}
