package dev.shynewt.shyspace.features.onboarding

import android.Manifest
import android.util.Log
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.compose.CameraXViewfinder
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.core.SurfaceRequest
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.lifecycle.awaitInstance
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import dev.shynewt.shyspace.ui.theme.ShyTheme
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.awaitCancellation
import zxingcpp.BarcodeReader

/**
 * "Scan code": asks for the camera only when tapped, and reads QR codes until one is a link code
 * ([isLinkCode]); others are skipped. Refused, or with no camera to use, it says to paste the code
 * instead; a phone with no camera at all shows no button. [scanner] is the camera, full screen.
 */
@Composable
fun ScanCodeButton(
    isLinkCode: (String) -> Boolean,
    onCode: (String) -> Unit,
    modifier: Modifier = Modifier,
    scanner: @Composable (isLinkCode: (String) -> Boolean, onCode: (String) -> Unit, onUnavailable: () -> Unit, onClose: () -> Unit) -> Unit =
        { accepts, found, unavailable, close -> CodeScanner(accepts, found, unavailable, close) },
) {
    val context = LocalContext.current
    if (!context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)) return
    var isScanning by rememberSaveable { mutableStateOf(false) }
    var problem by rememberSaveable { mutableStateOf<Int?>(null) }
    val askForCamera = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { isGranted ->
        isScanning = isGranted
        problem = if (isGranted) null else R.string.camera_refused
    }
    OutlinedButton(
        onClick = {
            val hasCamera = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
            if (hasCamera) isScanning = true else askForCamera.launch(Manifest.permission.CAMERA)
        },
        modifier = modifier,
    ) { Text(stringResource(R.string.scan_code)) }
    problem?.let { line ->
        Text(
            stringResource(line),
            style = MaterialTheme.typography.bodyMedium,
            color = LocalShyColors.current.palette.textMuted,
            modifier = Modifier.announcedWhenItAppears(),
        )
    }
    if (isScanning) {
        scanner(
            isLinkCode,
            { code ->
                isScanning = false
                onCode(code)
            },
            {
                isScanning = false
                problem = R.string.camera_unavailable
            },
            { isScanning = false },
        )
    }
}

/** The first of the texts a frame's QR codes hold that is a link code, skipping any other. */
internal fun firstLinkCode(texts: List<String?>, isLinkCode: (String) -> Boolean): String? =
    texts.firstOrNull { text -> !text.isNullOrBlank() && isLinkCode(text) }

/**
 * The back camera, full screen, until it reads a link code. Frames are read on the phone and
 * dropped. [onUnavailable] when there's no back camera to use.
 */
@Composable
private fun CodeScanner(isLinkCode: (String) -> Boolean, onCode: (String) -> Unit, onUnavailable: () -> Unit, onClose: () -> Unit) {
    Dialog(onDismissRequest = onClose, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        val context = LocalContext.current
        val lifecycleOwner = LocalLifecycleOwner.current
        var surfaceRequest by remember { mutableStateOf<SurfaceRequest?>(null) }
        val currentOnCode by rememberUpdatedState(onCode)
        val currentIsLinkCode by rememberUpdatedState(isLinkCode)
        val currentOnUnavailable by rememberUpdatedState(onUnavailable)
        LaunchedEffect(lifecycleOwner) {
            val camera = ProcessCameraProvider.awaitInstance(context)
            val preview = Preview.Builder().build().apply { setSurfaceProvider { request -> surfaceRequest = request } }
            val reader = BarcodeReader(BarcodeReader.Options(formats = setOf(BarcodeReader.Format.QR_CODE)))
            val hasFound = AtomicBoolean(false)
            val analyzing = Executors.newSingleThreadExecutor()
            val mainThread = ContextCompat.getMainExecutor(context)
            val analysis = ImageAnalysis.Builder()
                .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                .build()
                .apply {
                    setAnalyzer(analyzing) { frame ->
                        val code = frame.use { firstLinkCode(reader.read(it).map { result -> result.text }, currentIsLinkCode) }
                        if (code != null && hasFound.compareAndSet(false, true)) mainThread.execute { currentOnCode(code) }
                    }
                }
            try {
                camera.bindToLifecycle(lifecycleOwner, CameraSelector.DEFAULT_BACK_CAMERA, preview, analysis)
            } catch (unavailable: IllegalArgumentException) {
                // No back camera (a tablet with only a front one, or one that's busy elsewhere).
                Log.w("CodeScanner", "no back camera to scan with", unavailable)
                analyzing.shutdown()
                currentOnUnavailable()
                return@LaunchedEffect
            }
            try {
                awaitCancellation()
            } finally {
                camera.unbind(preview, analysis)
                analyzing.shutdown()
            }
        }
        // Always dark: a camera view, like the system's own.
        ShyTheme(isDark = true) {
            CameraOverlay(surfaceRequest, onClose)
        }
    }
}

@Composable
private fun CameraOverlay(surfaceRequest: SurfaceRequest?, onClose: () -> Unit) {
    val colors = LocalShyColors.current.palette
    Box(modifier = Modifier.fillMaxSize().background(colors.bg)) {
        surfaceRequest?.let { request -> CameraXViewfinder(surfaceRequest = request, modifier = Modifier.fillMaxSize()) }
        Text(
            stringResource(R.string.scan_hint),
            style = MaterialTheme.typography.bodyLarge,
            color = colors.text,
            textAlign = TextAlign.Center,
            modifier = Modifier.align(Alignment.BottomCenter).safeDrawingPadding().padding(24.dp),
        )
        IconButton(onClick = onClose, modifier = Modifier.align(Alignment.TopStart).safeDrawingPadding().padding(8.dp)) {
            Icon(painterResource(R.drawable.ic_close), contentDescription = stringResource(R.string.close), tint = colors.text)
        }
    }
}
