package dev.eightbitdo.mobile.ble

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import java.util.UUID
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow

/** One controller found by [BleScanner]. */
data class DiscoveredDevice(
    val address: String,
    val name: String,
    val rssi: Int,
    val device: BluetoothDevice,
)

/**
 * Scans for the controller's App Mode advertisement.
 *
 * iOS only exposes the service UUID / advertised name, and the same is true of
 * most Android stack versions, so the scan is unfiltered and the matching
 * happens here. The flow stops the scan when its collector cancels.
 */
class BleScanner(private val context: Context) {

    private val adapter: BluetoothAdapter? =
        (context.getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager)?.adapter

    fun scan(): Flow<List<DiscoveredDevice>> = callbackFlow {
        val scanner = adapter?.bluetoothLeScanner
        if (adapter == null || !adapter.isEnabled || scanner == null) {
            close(IllegalStateException("Bluetooth is off or unavailable"))
            return@callbackFlow
        }

        val found = linkedMapOf<String, DiscoveredDevice>()
        val callback = object : ScanCallback() {
            override fun onScanResult(callbackType: Int, result: ScanResult) {
                val device = result.device ?: return
                val name = result.scanRecord?.deviceName ?: device.name
                val advertisedServices =
                    result.scanRecord?.serviceUuids?.map { it.uuid } ?: emptyList()
                val matchesName = name?.startsWith(ADVERTISED_NAME_PREFIX) == true
                val matchesService = advertisedServices.contains(APP_SERVICE_UUID)
                if (!matchesName && !matchesService) return

                found[device.address] = DiscoveredDevice(
                    address = device.address,
                    name = name ?: "Unnamed controller",
                    rssi = result.rssi,
                    device = device,
                )
                trySend(found.values.toList())
            }

            override fun onScanFailed(errorCode: Int) {
                close(IllegalStateException("BLE scan failed with error code $errorCode"))
            }
        }

        val settings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .build()
        scanner.startScan(null, settings, callback)
        awaitClose { scanner.stopScan(callback) }
    }

    companion object {
        /** Advertised name prefix of the App Mode peripheral. */
        const val ADVERTISED_NAME_PREFIX = "80ND"

        /** App Mode GATT service. */
        val APP_SERVICE_UUID: UUID = UUID.fromString("0000FF10-0000-1000-8000-00805F9B34FB")
    }
}
