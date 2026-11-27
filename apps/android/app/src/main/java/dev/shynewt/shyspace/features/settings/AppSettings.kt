package dev.shynewt.shyspace.features.settings

import androidx.datastore.core.handlers.ReplaceFileCorruptionHandler
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.preferencesOf
import java.io.File
import java.io.IOException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking

/**
 * Settings kept on this phone only, never synced, in [file]. Read once when made, so the first
 * frame already follows them. A file that's damaged or can't be read hides the screen content,
 * the safe side, rather than stopping the app from starting.
 */
class AppSettings(file: File, private val scope: CoroutineScope) {
    private val store = PreferenceDataStoreFactory.create(
        corruptionHandler = ReplaceFileCorruptionHandler { preferencesOf(HIDES_SCREEN_CONTENT to true) },
        scope = scope,
        produceFile = { file },
    )

    private val hides = store.data
        .map { it[HIDES_SCREEN_CONTENT] ?: false }
        .catch { error -> if (error is IOException) emit(true) else throw error }

    /** Whether screenshots, screen recording and the recent-apps preview show nothing of the app. */
    val hidesScreenContent: StateFlow<Boolean> = hides.stateIn(scope, SharingStarted.Eagerly, runBlocking { hides.first() })

    private val mutableSaveFailed = MutableStateFlow(false)

    /** Whether the last change couldn't be written; the setting then stays as it was. */
    val saveFailed: StateFlow<Boolean> = mutableSaveFailed.asStateFlow()

    fun setHidesScreenContent(hides: Boolean) {
        scope.launch {
            mutableSaveFailed.value = try {
                store.edit { it[HIDES_SCREEN_CONTENT] = hides }
                false
            } catch (error: IOException) {
                true
            }
        }
    }

    private companion object {
        val HIDES_SCREEN_CONTENT = booleanPreferencesKey("hides_screen_content")
    }
}
