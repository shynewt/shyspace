package dev.shynewt.shyspace.features.network

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.content.ContextCompat
import dev.shynewt.shyspace.R
import kotlinx.coroutines.CompletableDeferred

/** Asks for the local network permission when something needs it, saying why before Android asks. */
@Stable
class LocalNetworkAccess internal constructor(private val context: Context) {
    internal var reasonAnswer by mutableStateOf<CompletableDeferred<Boolean>?>(null)
    internal var isShowingRefusal by mutableStateOf(false)
    internal lateinit var systemPrompt: ActivityResultLauncher<String>
    private var systemAnswer: CompletableDeferred<Boolean>? = null

    private val isGranted: Boolean
        get() = Build.VERSION.SDK_INT < LOCAL_NETWORK_SDK ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_LOCAL_NETWORK) == PackageManager.PERMISSION_GRANTED

    /**
     * Whether the app may reach the local network: at once if it may, otherwise after explaining
     * and asking. A refusal ends with how to allow it in Settings, since Android stops asking
     * after the second.
     */
    suspend fun allow(): Boolean {
        if (isGranted) return true
        val wantsToAsk = CompletableDeferred<Boolean>().also { reasonAnswer = it }.await()
        reasonAnswer = null
        if (!wantsToAsk) return false
        val granted = CompletableDeferred<Boolean>().also {
            systemAnswer = it
            systemPrompt.launch(Manifest.permission.ACCESS_LOCAL_NETWORK)
        }.await()
        isShowingRefusal = !granted
        return granted
    }

    internal fun onSystemAnswer(granted: Boolean) {
        systemAnswer?.complete(granted)
        systemAnswer = null
    }

    internal fun openAppSettings() {
        context.startActivity(
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }
}

/** Android 17, which brought the local network permission. */
private const val LOCAL_NETWORK_SDK = 37

/** Its prompts must be in the composition wherever [LocalNetworkAccess.allow] may be called. */
@Composable
fun rememberLocalNetworkAccess(): LocalNetworkAccess {
    val context = LocalContext.current
    val access = remember(context) { LocalNetworkAccess(context) }
    access.systemPrompt = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission(), access::onSystemAnswer)
    access.reasonAnswer?.let { answer ->
        LocalNetworkReason(onContinue = { answer.complete(true) }, onDismiss = { answer.complete(false) })
    }
    if (access.isShowingRefusal) {
        LocalNetworkRefused(
            onOpenSettings = {
                access.isShowingRefusal = false
                access.openAppSettings()
            },
            onDismiss = { access.isShowingRefusal = false },
        )
    }
    return access
}

@Composable
internal fun LocalNetworkReason(onContinue: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.local_network_title)) },
        text = { Text(stringResource(R.string.local_network_reason)) },
        confirmButton = { TextButton(onClick = onContinue) { Text(stringResource(R.string.continue_action)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.not_now)) } },
    )
}

@Composable
private fun LocalNetworkRefused(onOpenSettings: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.local_network_refused_title)) },
        text = { Text(stringResource(R.string.local_network_refused)) },
        confirmButton = { TextButton(onClick = onOpenSettings) { Text(stringResource(R.string.open_settings)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.close)) } },
    )
}
