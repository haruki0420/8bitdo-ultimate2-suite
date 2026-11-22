package dev.eightbitdo.mobile

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.eightbitdo.mobile.ble.BleScanner
import dev.eightbitdo.mobile.ble.BluetoothGattTransport
import dev.eightbitdo.mobile.ble.DiscoveredDevice
import dev.eightbitdo.mobile.core.DeviceConfig
import dev.eightbitdo.mobile.core.DeviceInfo
import dev.eightbitdo.mobile.core.MacroBackupView
import dev.eightbitdo.mobile.core.MobileSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Everything the single screen renders. */
data class UiState(
    val status: String = "未接続",
    val devices: List<DiscoveredDevice> = emptyList(),
    val deviceInfo: DeviceInfo? = null,
    val config: DeviceConfig? = null,
    val macros: MacroBackupView? = null,
    val busy: Boolean = false,
    val connected: Boolean = false,
    val error: String? = null,
)

/**
 * Drives scan → connect → read for the BLE-only first milestone.
 *
 * Rust calls block, so every [MobileSession] call runs on
 * [Dispatchers.IO]. Writes are intentionally not exposed: the configuration
 * write path (`Session::write_full_config`) needs the backup + confirmation
 * flow and a Tier 3 review first.
 */
class DeviceViewModel(application: Application) : AndroidViewModel(application) {

    private val scanner = BleScanner(application)
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    private var scanJob: Job? = null
    private var session: MobileSession? = null
    private var transport: BluetoothGattTransport? = null

    fun startScan() {
        scanJob?.cancel()
        scanJob = viewModelScope.launch {
            _state.value = _state.value.copy(status = "スキャン中…", error = null, devices = emptyList())
            try {
                scanner.scan().collect { devices ->
                    _state.value = _state.value.copy(devices = devices)
                }
            } catch (error: Exception) {
                _state.value = _state.value.copy(
                    status = "未接続",
                    error = error.message ?: error.toString(),
                )
            }
        }
    }

    fun stopScan() {
        scanJob?.cancel()
        scanJob = null
        if (!_state.value.connected) {
            _state.value = _state.value.copy(status = "未接続")
        }
    }

    fun connect(device: DiscoveredDevice) {
        scanJob?.cancel()
        scanJob = null
        viewModelScope.launch {
            _state.value = _state.value.copy(
                status = "接続中: ${device.name}",
                busy = true,
                error = null,
            )
            try {
                val newTransport = BluetoothGattTransport(getApplication(), device.device)
                withContext(Dispatchers.IO) { newTransport.open() }
                val newSession = withContext(Dispatchers.IO) { MobileSession(newTransport) }
                transport = newTransport
                session = newSession
                _state.value = _state.value.copy(
                    status = "接続済み: ${device.name}",
                    connected = true,
                    busy = false,
                    devices = emptyList(),
                )
                readAll()
            } catch (error: Exception) {
                _state.value = _state.value.copy(
                    status = "未接続",
                    connected = false,
                    busy = false,
                    error = error.message ?: error.toString(),
                )
            }
        }
    }

    fun readAll() {
        val current = session ?: return
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, error = null)
            try {
                val info = withContext(Dispatchers.IO) { current.readDeviceInfo() }
                _state.value = _state.value.copy(deviceInfo = info)
                val config = withContext(Dispatchers.IO) { current.readConfig() }
                _state.value = _state.value.copy(config = config)
            } catch (error: Exception) {
                _state.value = _state.value.copy(error = error.message ?: error.toString())
            } finally {
                _state.value = _state.value.copy(busy = false)
            }
        }
    }

    fun readMacroBackup() {
        val current = session ?: return
        viewModelScope.launch {
            _state.value = _state.value.copy(busy = true, error = null)
            try {
                val backup = withContext(Dispatchers.IO) { current.readMacroBackup() }
                _state.value = _state.value.copy(macros = backup)
            } catch (error: Exception) {
                _state.value = _state.value.copy(error = error.message ?: error.toString())
            } finally {
                _state.value = _state.value.copy(busy = false)
            }
        }
    }

    fun disconnect() {
        val current = session
        session = null
        transport = null
        viewModelScope.launch {
            withContext(Dispatchers.IO) { current?.disconnect() }
            _state.value = UiState()
        }
    }

    override fun onCleared() {
        super.onCleared()
        session?.disconnect()
        session = null
    }
}
