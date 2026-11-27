package dev.shynewt.shyspace.features.settings

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SegmentedListItem
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.SectionLabel
import dev.shynewt.shyspace.ui.theme.LocalShyColors

/** The You tab: this phone's settings. */
@Composable
fun YouScreen(
    hidesScreenContent: Boolean,
    onHidesScreenContentChange: (Boolean) -> Unit,
    saveFailed: Boolean,
    bottomBar: @Composable () -> Unit,
) {
    val colors = LocalShyColors.current.palette
    Scaffold(
        containerColor = colors.bg,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        stringResource(R.string.you),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.semantics { heading() },
                    )
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = colors.bg),
            )
        },
        bottomBar = bottomBar,
    ) { padding ->
        Column(
            modifier = Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp),
        ) {
            SectionLabel(stringResource(R.string.privacy), modifier = Modifier.padding(start = 16.dp))
            // A setting, not a selection: only the switch shows its state, so the card stays plain.
            SegmentedListItem(
                shapes = ListItemDefaults.segmentedShapes(index = 0, count = 1),
                colors = ListItemDefaults.colors(containerColor = colors.surface),
                modifier = Modifier.toggleable(
                    value = hidesScreenContent,
                    role = Role.Switch,
                    onValueChange = onHidesScreenContentChange,
                ),
                supportingContent = { Text(stringResource(R.string.hide_screen_content_detail)) },
                trailingContent = { Switch(checked = hidesScreenContent, onCheckedChange = null) },
            ) {
                Text(stringResource(R.string.hide_screen_content))
            }
            if (saveFailed) {
                Text(
                    stringResource(R.string.setting_not_saved),
                    style = MaterialTheme.typography.bodyMedium,
                    color = colors.dangerText,
                    modifier = Modifier.padding(start = 16.dp, top = 8.dp).semantics { liveRegion = LiveRegionMode.Polite },
                )
            }
        }
    }
}
