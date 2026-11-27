package dev.shynewt.shyspace.features.onboarding

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.Logo
import dev.shynewt.shyspace.ui.theme.LocalShyColors
import kotlinx.coroutines.launch

/** Readable line length for a phone held sideways or a tablet. */
private val CONTENT_WIDTH = 480.dp

/**
 * The first screen on a phone with no workspace. [onLinkThisPhone] is null until linking is
 * available; [developerTools] only development builds have.
 */
@Composable
fun WelcomeScreen(onLinkThisPhone: (() -> Unit)?, developerTools: (@Composable () -> Unit)?, createError: String?) {
    val colors = LocalShyColors.current.palette
    var isShowingTools by rememberSaveable { mutableStateOf(false) }
    Scaffold(containerColor = colors.bg, contentColor = colors.text) { padding ->
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
                .imePadding()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp, vertical = 32.dp),
        ) {
            Spacer(modifier = Modifier.height(48.dp))
            Logo(size = 72.dp)
            Spacer(modifier = Modifier.height(20.dp))
            Text(
                stringResource(R.string.app_name),
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.SemiBold,
                color = colors.text,
                modifier = Modifier.semantics { heading() },
            )
            Spacer(modifier = Modifier.height(8.dp))
            Text(
                stringResource(R.string.tagline),
                style = MaterialTheme.typography.bodyLarge,
                color = colors.textMuted,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = CONTENT_WIDTH),
            )
            Spacer(modifier = Modifier.height(40.dp))
            Column(
                verticalArrangement = Arrangement.spacedBy(8.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                modifier = Modifier.widthIn(max = CONTENT_WIDTH).fillMaxWidth(),
            ) {
                if (onLinkThisPhone != null) {
                    Button(onClick = onLinkThisPhone, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.already_use)) }
                    Text(
                        stringResource(R.string.already_use_detail),
                        style = MaterialTheme.typography.bodyMedium,
                        color = colors.textMuted,
                        textAlign = TextAlign.Center,
                    )
                } else {
                    Text(stringResource(R.string.no_workspace), style = MaterialTheme.typography.bodyMedium, color = colors.textMuted)
                }
                if (developerTools != null) {
                    Spacer(modifier = Modifier.height(24.dp))
                    TextButton(onClick = { isShowingTools = !isShowingTools }) { Text(stringResource(R.string.for_developers)) }
                    if (isShowingTools) developerTools()
                }
                createError?.let { error ->
                    Text(error, style = MaterialTheme.typography.bodyMedium, color = colors.dangerText)
                }
            }
        }
    }
}

/** Linking this phone to an account: paste (or later scan) the code, then the other device confirms. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkScreen(step: LinkStep, onCodeChange: (String) -> Unit, onStartAgain: () -> Unit, onBack: () -> Unit) {
    val colors = LocalShyColors.current.palette
    Scaffold(
        containerColor = colors.bg,
        contentColor = colors.text,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        stringResource(R.string.link_this_phone),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.semantics { heading() },
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(painterResource(R.drawable.ic_arrow_back), contentDescription = stringResource(R.string.back))
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = colors.bg, scrolledContainerColor = colors.bg),
            )
        },
    ) { padding ->
        Box(modifier = Modifier.fillMaxSize().padding(padding).imePadding(), contentAlignment = Alignment.TopCenter) {
            Column(
                verticalArrangement = Arrangement.spacedBy(16.dp),
                modifier = Modifier
                    .widthIn(max = CONTENT_WIDTH)
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = 24.dp, vertical = 16.dp),
            ) {
                when (step) {
                    is LinkStep.EnterCode -> EnterCode(step.hasExpired, onCodeChange)
                    is LinkStep.Checking -> Checking(step.checkCode)
                    LinkStep.SomeoneElse -> {
                        Note(stringResource(R.string.link_someone_else))
                        OutlinedButton(onClick = onStartAgain, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.start_again)) }
                    }
                    LinkStep.Stopped -> {
                        Note(stringResource(R.string.link_stopped))
                        OutlinedButton(onClick = onStartAgain, modifier = Modifier.fillMaxWidth()) { Text(stringResource(R.string.start_again)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun EnterCode(hasExpired: Boolean, onCodeChange: (String) -> Unit) {
    val colors = LocalShyColors.current.palette
    val code = rememberTextFieldState()
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    // A whole code is taken as it lands: there's no Continue button.
    LaunchedEffect(code) { snapshotFlow { code.text.toString() }.collect(onCodeChange) }
    Text(stringResource(R.string.link_where), style = MaterialTheme.typography.bodyLarge, color = colors.text)
    Text(stringResource(R.string.link_gets), style = MaterialTheme.typography.bodyMedium, color = colors.textMuted)
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        CodeField(code, modifier = Modifier.weight(1f))
        OutlinedButton(onClick = {
            scope.launch {
                val pasted = clipboard.getClipEntry()?.clipData?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text ?: return@launch
                code.setTextAndPlaceCursorAtEnd(pasted.toString().trim())
            }
        }) { Text(stringResource(R.string.paste)) }
    }
    if (hasExpired) {
        Text(
            stringResource(R.string.link_code_expired),
            style = MaterialTheme.typography.bodyMedium,
            color = colors.dangerText,
            modifier = Modifier.announcedWhenItAppears(),
        )
    }
}

@Composable
private fun CodeField(code: TextFieldState, modifier: Modifier) {
    val label = stringResource(R.string.link_code)
    OutlinedTextField(
        state = code,
        label = { Text(label) },
        placeholder = { Text(stringResource(R.string.paste_code)) },
        lineLimits = TextFieldLineLimits.SingleLine,
        // A password keyboard, shown as plain text: no autocorrect, no suggestions, and the
        // keyboard doesn't learn the code.
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        modifier = modifier,
    )
}

@Composable
private fun Checking(checkCode: String) {
    val colors = LocalShyColors.current.palette
    // Read out as two groups, as people say it ("482, 917").
    // The core may space the number ("482 917"); group the digits alone.
    val groups = checkCode.filter(Char::isDigit).chunked(3)
    Text(
        stringResource(R.string.link_tap_this),
        style = MaterialTheme.typography.titleMedium,
        color = colors.text,
        modifier = Modifier.semantics { heading() }.announcedWhenItAppears(),
    )
    Text(
        groups.joinToString(" "),
        style = MaterialTheme.typography.displayMedium,
        fontFamily = FontFamily.Monospace,
        fontWeight = FontWeight.SemiBold,
        color = colors.text,
        textAlign = TextAlign.Center,
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 16.dp)
            .semantics { contentDescription = groups.joinToString(", ") }
            .announcedWhenItAppears(),
    )
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        CircularProgressIndicator(modifier = Modifier.size(18.dp), strokeWidth = 2.dp)
        Text(stringResource(R.string.link_waiting), style = MaterialTheme.typography.bodyMedium, color = colors.textMuted)
    }
}

/** A step's message, read out when it appears, since it comes on its own rather than from a tap. */
@Composable
private fun Note(text: String) {
    Text(text, style = MaterialTheme.typography.bodyLarge, color = LocalShyColors.current.palette.text, modifier = Modifier.announcedWhenItAppears())
}

/** A screen reader reads this when it appears, after what it's reading now. */
private fun Modifier.announcedWhenItAppears(): Modifier = semantics { liveRegion = LiveRegionMode.Polite }
