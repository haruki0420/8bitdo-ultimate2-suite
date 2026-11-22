package dev.eightbitdo.mobile.ble

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.Context
import android.os.Build
import dev.eightbitdo.mobile.core.NativeTransport
import dev.eightbitdo.mobile.core.NativeTransportException
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeout

/**
 * `BluetoothGatt` implementation of the Rust `NativeTransport` callback.
 *
 * The Rust bridge calls [recv] from its own receive thread and [send] from the
 * session thread, so frames cross the boundary through a blocking queue and all
 * GATT operations are serialised on a single-thread executor (Android rejects
 * concurrent GATT operations).
 *
 * The MTU is the value reported by `onMtuChanged`, never the requested one
 * (Android 14+ may pin it to 517).
 */
class BluetoothGattTransport(
    private val context: Context,
    private val device: BluetoothDevice,
) : NativeTransport {

    private val frames = LinkedBlockingQueue<ByteArray>()
    private val gattExecutor = Executors.newSingleThreadExecutor()

    @Volatile
    private var gatt: BluetoothGatt? = null

    @Volatile
    private var appCharacteristic: BluetoothGattCharacteristic? = null

    @Volatile
    private var mtuValue: UShort = 23u

    @Volatile
    private var closed = false

    @Volatile
    private var connectionSignal: CompletableDeferred<BluetoothGatt>? = null

    @Volatile
    private var servicesSignal: CompletableDeferred<Unit>? = null

    @Volatile
    private var descriptorSignal: CompletableDeferred<Unit>? = null

    @Volatile
    private var mtuSignal: CompletableDeferred<Int>? = null

    /**
     * Connect, discover the App Mode service and enable notifications.
     *
     * Suspend: called from a coroutine on [kotlinx.coroutines.Dispatchers.IO].
     */
    suspend fun open() {
        val connection = CompletableDeferred<BluetoothGatt>()
        connectionSignal = connection
        val connectedGatt = device.connectGatt(
            context,
            false,
            gattCallback,
            BluetoothDevice.TRANSPORT_LE,
        ) ?: throw NativeTransportException.Failure("connectGatt returned null")
        gatt = connectedGatt
        withTimeout(CONNECT_TIMEOUT_MS) { connection.await() }

        val services = CompletableDeferred<Unit>()
        servicesSignal = services
        if (!connectedGatt.discoverServices()) {
            throw NativeTransportException.Failure("discoverServices was refused")
        }
        withTimeout(DISCOVERY_TIMEOUT_MS) { services.await() }

        val service = connectedGatt.getService(APP_SERVICE_UUID)
            ?: throw NativeTransportException.Failure("App Mode service FF10 not found")
        val characteristic = service.getCharacteristic(APP_CHARACTERISTIC_UUID)
            ?: service.getCharacteristic(APP_CHARACTERISTIC_ALT_UUID)
            ?: throw NativeTransportException.Failure("App Mode characteristic FF11/FF13 not found")
        appCharacteristic = characteristic

        if (!connectedGatt.setCharacteristicNotification(characteristic, true)) {
            throw NativeTransportException.Failure("could not enable notifications")
        }
        val cccd = characteristic.getDescriptor(CCCD_UUID)
            ?: throw NativeTransportException.Failure("notification descriptor 0x2902 not found")
        val descriptor = CompletableDeferred<Unit>()
        descriptorSignal = descriptor
        writeDescriptor(connectedGatt, cccd)
        runCatching { withTimeout(CCCD_TIMEOUT_MS) { descriptor.await() } }

        val mtu = CompletableDeferred<Int>()
        mtuSignal = mtu
        connectedGatt.requestMtu(REQUESTED_MTU)
        // The negotiation is advisory: keep the default if the stack never reports.
        mtuValue = runCatching { withTimeout(MTU_TIMEOUT_MS) { mtu.await() } }
            .getOrDefault(23)
            .toUShort()
    }

    // ---- NativeTransport -------------------------------------------------

    override fun send(data: ByteArray) {
        if (closed) throw NativeTransportException.Failure("GATT link is closed")
        val result = CompletableFuture<Unit>()
        gattExecutor.execute {
            try {
                writeFrame(data)
                result.complete(Unit)
            } catch (error: Throwable) {
                result.completeExceptionally(error)
            }
        }
        try {
            result.get(WRITE_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (error: Exception) {
            throw NativeTransportException.Failure("GATT write failed: ${error.message}")
        }
    }

    override fun recv(timeoutMs: UInt): ByteArray? {
        val frame = frames.poll(timeoutMs.toLong(), TimeUnit.MILLISECONDS)
        if (frame == null && closed) {
            throw NativeTransportException.Failure("GATT link is closed")
        }
        return frame
    }

    override fun mtu(): UShort = mtuValue

    override fun disconnect() {
        if (closed) return
        closed = true
        val current = gatt
        gatt = null
        current?.let {
            runCatching { it.disconnect() }
            runCatching { it.close() }
        }
        gattExecutor.shutdownNow()
    }

    // ---- internals -------------------------------------------------------

    private fun writeFrame(data: ByteArray) {
        val currentGatt = gatt ?: throw NativeTransportException.Failure("GATT link is closed")
        val characteristic = appCharacteristic
            ?: throw NativeTransportException.Failure("App Mode characteristic is not ready")

        // Android refuses a write while the previous one is in flight; retry a
        // few times instead of dropping a chunk.
        repeat(WRITE_ATTEMPTS) { attempt ->
            val accepted = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                currentGatt.writeCharacteristic(
                    characteristic,
                    data,
                    BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE,
                ) == BluetoothStatusCodes.SUCCESS
            } else {
                @Suppress("DEPRECATION")
                run {
                    characteristic.value = data
                    currentGatt.writeCharacteristic(characteristic)
                }
            }
            if (accepted) return
            if (attempt < WRITE_ATTEMPTS - 1) {
                Thread.sleep(WRITE_RETRY_DELAY_MS)
            }
        }
        throw NativeTransportException.Failure("GATT write was refused after $WRITE_ATTEMPTS attempts")
    }

    private fun writeDescriptor(currentGatt: BluetoothGatt, descriptor: BluetoothGattDescriptor) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            currentGatt.writeDescriptor(
                descriptor,
                BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE,
            )
        } else {
            @Suppress("DEPRECATION")
            run {
                descriptor.value = BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE
                currentGatt.writeDescriptor(descriptor)
            }
        }
    }

    private fun failAllSignals(reason: String) {
        val error = NativeTransportException.Failure(reason)
        connectionSignal?.completeExceptionally(error)
        servicesSignal?.completeExceptionally(error)
        descriptorSignal?.completeExceptionally(error)
        mtuSignal?.completeExceptionally(error)
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(
            currentGatt: BluetoothGatt,
            status: Int,
            newState: Int,
        ) {
            when (newState) {
                BluetoothProfile.STATE_CONNECTED ->
                    connectionSignal?.complete(currentGatt)

                BluetoothProfile.STATE_DISCONNECTED -> {
                    closed = true
                    failAllSignals("connection lost (status $status)")
                }

                else -> Unit
            }
        }

        override fun onServicesDiscovered(currentGatt: BluetoothGatt, status: Int) {
            if (status == BluetoothGatt.GATT_SUCCESS) {
                servicesSignal?.complete(Unit)
            } else {
                servicesSignal?.completeExceptionally(
                    NativeTransportException.Failure("service discovery failed (status $status)"),
                )
            }
        }

        override fun onDescriptorWrite(
            currentGatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int,
        ) {
            if (descriptor.uuid != CCCD_UUID) return
            if (status == BluetoothGatt.GATT_SUCCESS) {
                descriptorSignal?.complete(Unit)
            } else {
                descriptorSignal?.completeExceptionally(
                    NativeTransportException.Failure("CCCD write failed (status $status)"),
                )
            }
        }

        override fun onMtuChanged(currentGatt: BluetoothGatt, mtu: Int, status: Int) {
            mtuValue = mtu.toUShort()
            if (status == BluetoothGatt.GATT_SUCCESS) {
                mtuSignal?.complete(mtu)
            } else {
                mtuSignal?.completeExceptionally(
                    NativeTransportException.Failure("MTU request failed (status $status)"),
                )
            }
        }

        override fun onCharacteristicChanged(
            currentGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            if (characteristic.uuid == APP_CHARACTERISTIC_UUID ||
                characteristic.uuid == APP_CHARACTERISTIC_ALT_UUID
            ) {
                frames.put(value)
            }
        }

        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(
            currentGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
        ) {
            if (characteristic.uuid == APP_CHARACTERISTIC_UUID ||
                characteristic.uuid == APP_CHARACTERISTIC_ALT_UUID
            ) {
                characteristic.value?.let { frames.put(it) }
            }
        }
    }

    companion object {
        /** App Mode GATT service (`0000FF10-...`). */
        val APP_SERVICE_UUID: UUID = UUID.fromString("0000FF10-0000-1000-8000-00805F9B34FB")

        /** Documented App Mode characteristic. */
        val APP_CHARACTERISTIC_UUID: UUID = UUID.fromString("0000FF11-0000-1000-8000-00805F9B34FB")

        /** Characteristic seen on some units instead of FF11. */
        val APP_CHARACTERISTIC_ALT_UUID: UUID =
            UUID.fromString("0000FF13-0000-1000-8000-00805F9B34FB")

        private val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805F9B34FB")

        private const val REQUESTED_MTU = 200
        private const val CONNECT_TIMEOUT_MS = 10_000L
        private const val DISCOVERY_TIMEOUT_MS = 10_000L
        private const val CCCD_TIMEOUT_MS = 3_000L
        private const val MTU_TIMEOUT_MS = 3_000L
        private const val WRITE_TIMEOUT_MS = 2_000L
        private const val WRITE_ATTEMPTS = 100
        private const val WRITE_RETRY_DELAY_MS = 10L
    }
}
