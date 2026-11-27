package dev.shynewt.shyspace.features.home

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.OutlinedSecureTextField
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import dev.shynewt.shyspace.R
import dev.shynewt.shyspace.features.network.endpointIsOnLocalNetwork
import dev.shynewt.shyspace.features.network.rememberLocalNetworkAccess
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.shy_ffi.BucketSettings

/**
 * Creates a workspace on a bucket, in development builds only: phones join as another device
 * of their person. Filled in with the local bucket from docs/development.md, as the emulator
 * reaches it. Release builds have an empty one (src/release).
 */
@Composable
fun DevelopmentWorkspaceTools(isCreating: Boolean, onCreate: (workspaceName: String, memberName: String, bucket: BucketSettings) -> Unit) {
    var workspaceName by rememberSaveable { mutableStateOf("Test") }
    var memberName by rememberSaveable { mutableStateOf("") }
    var endpoint by rememberSaveable { mutableStateOf("http://10.0.2.2:9000") }
    var region by rememberSaveable { mutableStateOf("us-east-1") }
    var bucket by rememberSaveable { mutableStateOf("shyspace") }
    var accessKey by rememberSaveable { mutableStateOf("shyadmin") }
    val secretKey = rememberTextFieldState("shysecret123")
    val fields = listOf(workspaceName, memberName, endpoint, region, bucket, accessKey, secretKey.text.toString())
    val scope = rememberCoroutineScope()
    val localNetwork = rememberLocalNetworkAccess()

    Column(verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
        Field(R.string.workspace_name, workspaceName) { workspaceName = it }
        Field(R.string.your_name, memberName) { memberName = it }
        Field(R.string.bucket_endpoint, endpoint, KeyboardType.Uri) { endpoint = it }
        Field(R.string.bucket_region, region) { region = it }
        Field(R.string.bucket_name, bucket) { bucket = it }
        Field(R.string.bucket_access_key, accessKey) { accessKey = it }
        OutlinedSecureTextField(
            state = secretKey,
            label = { Text(stringResource(R.string.bucket_secret_key)) },
            modifier = Modifier.fillMaxWidth(),
        )
        Button(
            onClick = {
                scope.launch {
                    val address = endpoint.trim()
                    val isLocal = withContext(Dispatchers.IO) { endpointIsOnLocalNetwork(address) }
                    if (isLocal && !localNetwork.allow()) return@launch
                    onCreate(
                        workspaceName.trim(),
                        memberName.trim(),
                        BucketSettings(address, region.trim(), bucket.trim(), accessKey.trim(), secretKey.text.toString()),
                    )
                }
            },
            enabled = !isCreating && fields.all { it.isNotBlank() },
            modifier = Modifier.fillMaxWidth(),
        ) { Text(stringResource(R.string.create_test_workspace)) }
    }
}

@Composable
private fun Field(label: Int, value: String, keyboardType: KeyboardType = KeyboardType.Text, onChange: (String) -> Unit) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(stringResource(label)) },
        keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
        singleLine = true,
        modifier = Modifier.fillMaxWidth(),
    )
}
