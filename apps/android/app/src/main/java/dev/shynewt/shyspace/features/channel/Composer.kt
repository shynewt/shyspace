package dev.shynewt.shyspace.features.channel

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.ui.theme.LocalShyColors

/**
 * The message box: [draft] grows up to six lines, and Send is filled once there's something to
 * send. While [isEditing], a line above says so and offers to stop.
 */
@Composable
fun Composer(
    placeholder: String,
    draft: TextFieldState,
    isEditing: Boolean,
    onCancelEdit: () -> Unit,
    onSubmit: () -> Unit,
) {
    val colors = LocalShyColors.current
    Column(modifier = Modifier.fillMaxWidth().navigationBarsPadding()) {
        if (isEditing) {
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 4.dp),
            ) {
                Icon(
                    painterResource(R.drawable.ic_edit),
                    contentDescription = null,
                    tint = colors.accent.accentText,
                    modifier = Modifier.size(16.dp),
                )
                Text(
                    stringResource(R.string.editing_message),
                    style = MaterialTheme.typography.labelLarge,
                    fontWeight = FontWeight.SemiBold,
                    color = colors.accent.accentText,
                    modifier = Modifier.weight(1f).padding(start = 8.dp),
                )
                IconButton(onClick = onCancelEdit) {
                    Icon(painterResource(R.drawable.ic_close), contentDescription = stringResource(R.string.cancel_edit))
                }
            }
        }
        Row(
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            modifier = Modifier.fillMaxWidth().padding(start = 12.dp, end = 8.dp, top = 6.dp, bottom = 8.dp),
        ) {
            BasicTextField(
                state = draft,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.palette.text),
                cursorBrush = SolidColor(colors.accent.accent),
                lineLimits = TextFieldLineLimits.MultiLine(maxHeightInLines = 6),
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 48.dp)
                    .semantics { contentDescription = placeholder },
                decorator = { field ->
                    Box(
                        contentAlignment = Alignment.CenterStart,
                        modifier = Modifier
                            .background(colors.palette.surface2, MaterialTheme.shapes.large)
                            .border(1.dp, if (isEditing) colors.accent.accent else colors.palette.border, MaterialTheme.shapes.large)
                            .padding(horizontal = 14.dp, vertical = 10.dp),
                    ) {
                        if (draft.text.isEmpty()) Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = colors.palette.textFaint)
                        field()
                    }
                },
            )
            FilledIconButton(
                onClick = onSubmit,
                enabled = draft.text.isNotBlank(),
                colors = IconButtonDefaults.filledIconButtonColors(
                    containerColor = colors.accent.accent,
                    contentColor = colors.accent.onAccent,
                    disabledContainerColor = colors.palette.surface2,
                    disabledContentColor = colors.palette.textDecorative,
                ),
                modifier = Modifier.padding(bottom = 2.dp),
            ) {
                Icon(
                    painterResource(R.drawable.ic_send),
                    contentDescription = stringResource(if (isEditing) R.string.save_edit else R.string.send),
                )
            }
        }
    }
}
