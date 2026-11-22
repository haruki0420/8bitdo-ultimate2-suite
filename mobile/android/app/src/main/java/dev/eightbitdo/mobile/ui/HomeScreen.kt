package dev.eightbitdo.mobile.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.collectAsState
import dev.eightbitdo.mobile.DeviceViewModel
import dev.eightbitdo.mobile.ble.DiscoveredDevice
import dev.eightbitdo.mobile.core.DeviceConfig
import dev.eightbitdo.mobile.core.DeviceInfo
import dev.eightbitdo.mobile.core.MacroEntryInfo

/**
 * Single screen for the read-only milestone: connection, firmware, config and
 * macro backup. Configuration editing/writing is deliberately absent until the
 * backup + confirmation flow exists.
 */
@Composable
fun HomeScreen(viewModel: DeviceViewModel) {
    val state by viewModel.state.collectAsState()

    LazyColumn(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item {
            Text("Ultimate 2 Mobile", style = MaterialTheme.typography.headlineSmall)
            Text(
                "非公式ツール / unofficial. BLE 読み取り専用（このマイルストーン）",
                style = MaterialTheme.typography.bodySmall,
            )
        }

        item {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(state.status, modifier = Modifier.weight(1f))
                        if (state.busy) {
                            CircularProgressIndicator(modifier = Modifier.padding(8.dp))
                        }
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        if (state.connected) {
                            OutlinedButton(onClick = viewModel::readAll) { Text("再読み込み") }
                            OutlinedButton(onClick = viewModel::disconnect) { Text("切断") }
                        } else {
                            OutlinedButton(
                                onClick = viewModel::startScan,
                                enabled = !state.coreUnavailable,
                            ) { Text("スキャン") }
                        }
                    }
                }
            }
        }

        if (state.coreUnavailable) {
            item {
                Card(modifier = Modifier.fillMaxWidth()) {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        Text(
                            "Rust コアがありません",
                            style = MaterialTheme.typography.titleMedium,
                            color = MaterialTheme.colorScheme.error,
                        )
                        Text(
                            state.coreMessage ?: "NDK ビルド版の APK が必要です。",
                            style = MaterialTheme.typography.bodySmall,
                        )
                    }
                }
            }
        }

        state.error?.let { message ->
            item {
                Card(modifier = Modifier.fillMaxWidth()) {
                    Text(
                        text = message,
                        color = MaterialTheme.colorScheme.error,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }
        }

        if (state.devices.isNotEmpty()) {
            item { Text("コントローラ", style = MaterialTheme.typography.titleMedium) }
            items(state.devices, key = { it.address }) { device ->
                DeviceRow(device = device, onConnect = { viewModel.connect(device) })
            }
        }

        state.deviceInfo?.let { info ->
            item { FirmwareCard(info) }
        }

        state.config?.let { config ->
            item { ConfigCard(config) }
        }

        item {
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("マクロ", style = MaterialTheme.typography.titleMedium)
                    val macros = state.macros
                    if (macros == null) {
                        Text("未読み込み", style = MaterialTheme.typography.bodySmall)
                    } else {
                        macros.entries.forEach { entry -> MacroRow(entry) }
                    }
                    OutlinedButton(
                        onClick = viewModel::readMacroBackup,
                        enabled = state.connected && !state.busy,
                    ) { Text("3 スロット × 4 マクロをバックアップ") }
                }
            }
        }
    }
}

@Composable
private fun DeviceRow(device: DiscoveredDevice, onConnect: () -> Unit) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f)) {
                Text(device.name, style = MaterialTheme.typography.bodyLarge)
                Text(
                    "${device.address} · ${device.rssi} dBm",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            OutlinedButton(onClick = onConnect) { Text("接続") }
        }
    }
}

@Composable
private fun FirmwareCard(info: DeviceInfo) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("ファームウェア", style = MaterialTheme.typography.titleMedium)
            Text(
                if (info.text.isNotEmpty()) info.text else binaryVersion(info.raw),
                style = MaterialTheme.typography.bodyLarge,
            )
            Text(
                "raw: " + info.raw.joinToString(" ") { byte -> "%02X".format(byte.toInt() and 0xFF) },
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

private fun binaryVersion(raw: ByteArray): String {
    if (raw.size != 4) return "—"
    var value = 0L
    for (index in 0..3) {
        value = value or ((raw[index].toLong() and 0xFF) shl (8 * index))
    }
    return "$value (binary)"
}

@Composable
private fun ConfigCard(config: DeviceConfig) {
    Card(modifier = Modifier.fillMaxWidth()) {
        Column(modifier = Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("設定", style = MaterialTheme.typography.titleMedium)
            Text(
                if (config.crcValid) {
                    "CRC OK (0x%04X)".format(config.computedCrc.toInt())
                } else {
                    "CRC 不一致: stored 0x%04X / computed 0x%04X".format(
                        config.storedCrc.toInt(),
                        config.computedCrc.toInt(),
                    )
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (config.crcValid) {
                    MaterialTheme.colorScheme.onSurface
                } else {
                    MaterialTheme.colorScheme.error
                },
            )
            Text("モード: ${config.summary.gamepadMode}", style = MaterialTheme.typography.bodyLarge)
            Text(
                "アクティブスロット: ${config.summary.currentSlot?.toString() ?: "—"}",
                style = MaterialTheme.typography.bodySmall,
            )
            config.summary.profileNames.forEachIndexed { index, name ->
                Text(
                    "プロファイル ${index + 1}: ${name.ifEmpty { "—" }}",
                    style = MaterialTheme.typography.bodySmall,
                )
            }
            Text(
                "raw: ${config.raw.size} bytes（送信前バックアップ: JSON は Rust API から取得）",
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

@Composable
private fun MacroRow(entry: MacroEntryInfo) {
    Text(
        "slot ${entry.slot} · macro ${entry.index.toInt() + 1}: " +
            if (entry.isEmpty) {
                "empty"
            } else {
                "${entry.name.ifEmpty { "unnamed" }} / ${entry.stepCount} steps"
            },
        style = MaterialTheme.typography.bodySmall,
    )
}
