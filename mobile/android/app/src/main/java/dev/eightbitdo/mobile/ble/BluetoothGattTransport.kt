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
import dev.eightbitdo.mobile.core.minimumReadMtu
import java.util.UUID
import java.util.concurrent.CompletableFuture
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.withTimeout

/**
 * `BluetoothGatt` implementation of the Rust `NativeTransport` callback.
 *
 * The Rust bridge calls [recv] from its own receive thread and [send] from the
 * session thread, so frames cross the boundary through a bounded [FrameQueue]
 * and all GATT operations are serialised on a single-thread executor (Android
 * rejects concurrent GATT operations).
 *
 * # Failure handling
 *
 * Every asynchronous step (connect, service discovery, CCCD write, MTU) is
 * awaited strictly and an error closes the link instead of falling back to a
 * default. In particular the MTU must be >= [minimumReadMtu]: at the default 23
 * the stack cannot deliver the controller's 158-byte configuration
 * notifications, so a session created at that MTU would hang mid-read.
 *
 * # Lifecycle
 *
 * [open] either leaves a fully usable link or throws with all resources
 * released. [disconnect] is idempotent; a disconnection reported by the stack
 * also releases the GATT client and the executor, so a reconnect cannot leak
 * the previous client.
 */
class BluetoothGattTransport(
    private val context: Context,
    private val device: BluetoothDevice,
) : NativeTransport {

    private val frames = FrameQueue(FRAME_QUEUE_CAPACITY)
    private val gattExecutor = Executors.newSingleThreadExecutor()

    @Volatile
    private var gatt: BluetoothGatt? = null

    @Volatile
    private var appCharacteristic: BluetoothGattCharacteristic? = null

    @Volatile
    private var writeMode: AppWriteMode? = null

    @Volatile
    private var pendingWrite: PendingWrite? = null

    /**
     * Set after a with-response write times out or fails: a late
     * `onCharacteristicWrite` could otherwise be attributed to the next write.
     * Further writes are refused until the link is reconnected.
     */
    @Volatile
    private var writeNeedsReconnect = false

    @Volatile
    private var mtuValue: UShort = 23u

    @Volatile
    private var closed = false

    @Volatile
    private var released = false

    @Volatile
    private var connectionSignal: CompletableDeferred<BluetoothGatt>? = null

    @Volatile
    private var servicesSignal: CompletableDeferred<Unit>? = null

    @Volatile
    private var descriptorSignal: CompletableDeferred<Unit>? = null

    @Volatile
    private var mtuSignal: CompletableDeferred<Int>? = null

    /**
     * Connect, discover the App Mode service, enable notifications and
     * negotiate the MTU.
     *
     * Suspend: called from a coroutine on [kotlinx.coroutines.Dispatchers.IO].
     * On any failure the link, GATT client and executor are released before the
     * exception is rethrown.
     */
    suspend fun open() {
        try {
            openInternal()
        } catch (error: Throwable) {
            releaseResources()
            throw error
        }
    }

    private suspend fun openInternal() {
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

        // The protocol needs notifications and a writable characteristic.
        // Write-without-response is the normal mode, but some FF11/FF13
        // implementations only advertise write-with-response; both produce the
        // same frames, so accept either and fail only when neither exists.
        if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY == 0) {
            throw NativeTransportException.Failure("App Mode characteristic cannot notify")
        }
        writeMode = AppWriteMode.select(characteristic.properties)
            ?: throw NativeTransportException.Failure(
                "App Mode characteristic is not writable (no write property)",
            )
        appCharacteristic = characteristic

        if (!connectedGatt.setCharacteristicNotification(characteristic, true)) {
            throw NativeTransportException.Failure("could not enable notifications")
        }
        val cccd = characteristic.getDescriptor(CCCD_UUID)
            ?: throw NativeTransportException.Failure("notification descriptor 0x2902 not found")
        val descriptor = CompletableDeferred<Unit>()
        descriptorSignal = descriptor
        writeDescriptor(connectedGatt, cccd)
        withTimeout(CCCD_TIMEOUT_MS) { descriptor.await() }

        val mtu = CompletableDeferred<Int>()
        mtuSignal = mtu
        if (!connectedGatt.requestMtu(REQUESTED_MTU)) {
            throw NativeTransportException.Failure("requestMtu($REQUESTED_MTU) was refused")
        }
        val negotiated = withTimeout(MTU_TIMEOUT_MS) { mtu.await() }
        val minimum = minimumReadMtu().toInt()
        if (negotiated < minimum) {
            throw NativeTransportException.Failure(
                "negotiated MTU $negotiated is below the $minimum needed to read the configuration",
            )
        }
        mtuValue = negotiated.toUShort()
    }

    // ---- NativeTransport -------------------------------------------------

    override fun send(data: ByteArray) {
        if (closed) throw NativeTransportException.Failure("GATT link is closed")
        val characteristic = appCharacteristic
            ?: throw NativeTransportException.Failure("App Mode characteristic is not ready")
        val mode = writeMode
            ?: throw NativeTransportException.Failure("App Mode write mode is unknown")

        val result = CompletableFuture<Unit>()
        try {
            gattExecutor.execute {
                try {
                    writeFrame(data, characteristic, mode)
                    result.complete(Unit)
                } catch (error: Throwable) {
                    result.completeExceptionally(error)
                }
            }
        } catch (error: RejectedExecutionException) {
            throw NativeTransportException.Failure("GATT link is closed")
        }
        try {
            result.get(WRITE_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        } catch (error: Exception) {
            throw NativeTransportException.Failure("GATT write failed: ${error.message}")
        }
    }

    override fun recv(timeoutMs: UInt): ByteArray? {
        val frame = frames.poll(timeoutMs.toLong())
        if (frame == null && (closed || frames.isClosed())) {
            throw NativeTransportException.Failure("GATT link is closed")
        }
        return frame
    }

    override fun mtu(): UShort = mtuValue

    override fun disconnect() {
        // No early return on `closed`: a second call must still be able to
        // free resources left behind by a partial `open()`.
        releaseResources()
    }

    // ---- internals -------------------------------------------------------

    private fun releaseResources() {
        synchronized(this) {
            if (released) return
            released = true
        }
        closed = true
        writeNeedsReconnect = false
        frames.close()
        failAllSignals("GATT link closed")
        val current = gatt
        gatt = null
        current?.let {
            runCatching { it.disconnect() }
            runCatching { it.close() }
        }
        gattExecutor.shutdownNow()
    }

    private fun writeFrame(
        data: ByteArray,
        characteristic: BluetoothGattCharacteristic,
        mode: AppWriteMode,
    ) {
        val currentGatt = gatt ?: throw NativeTransportException.Failure("GATT link is closed")
        if (mode == AppWriteMode.WITH_RESPONSE) {
            writeWithResponse(currentGatt, characteristic, data)
        } else {
            writeWithoutResponse(currentGatt, characteristic, data)
        }
    }

    /** Write without an ATT response; retry while the stack is busy. */
    private fun writeWithoutResponse(
        currentGatt: BluetoothGatt,
        characteristic: BluetoothGattCharacteristic,
        data: ByteArray,
    ) {
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
                    characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
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

    /** Write with an ATT response and wait for `onCharacteristicWrite`. */
    private fun writeWithResponse(
        currentGatt: BluetoothGatt,
        characteristic: BluetoothGattCharacteristic,
        data: ByteArray,
    ) {
        if (writeNeedsReconnect) {
            throw NativeTransportException.Failure(
                "a previous write response was lost; reconnect before writing again",
            )
        }
        val pending = PendingWrite()
        pendingWrite = pending
        val issued = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            currentGatt.writeCharacteristic(
                characteristic,
                data,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT,
            ) == BluetoothStatusCodes.SUCCESS
        } else {
            @Suppress("DEPRECATION")
            run {
                characteristic.writeType = BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
                characteristic.value = data
                currentGatt.writeCharacteristic(characteristic)
            }
        }
        if (!issued) {
            pendingWrite = null
            throw NativeTransportException.Failure("GATT write (with response) was refused")
        }
        val completed = pending.done.await(WRITE_TIMEOUT_MS, TimeUnit.MILLISECONDS)
        pendingWrite = null
        if (!completed) {
            writeNeedsReconnect = true
            throw NativeTransportException.Failure("timed out waiting for the ATT write response")
        }
        if (pending.status != BluetoothGatt.GATT_SUCCESS) {
            writeNeedsReconnect = true
            throw NativeTransportException.Failure("GATT write failed (status ${pending.status})")
        }
    }

    /** Per-write completion signal for write-with-response. */
    private class PendingWrite {
        val done = CountDownLatch(1)

        @Volatile
        var status: Int = Int.MIN_VALUE
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
                    // The stack already dropped the link; release the client
                    // and executor so a reconnect starts clean. This also
                    // completes every pending signal with an error.
                    releaseResources()
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
            if (status != BluetoothGatt.GATT_SUCCESS) {
                // No fallback: the session requires a negotiated MTU that can
                // carry the device's 158-byte notifications.
                mtuSignal?.completeExceptionally(
                    NativeTransportException.Failure("MTU request failed (status $status)"),
                )
                return
            }
            mtuValue = mtu.toUShort()
            mtuSignal?.complete(mtu)
        }

        override fun onCharacteristicWrite(
            currentGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            if (characteristic.uuid != APP_CHARACTERISTIC_UUID &&
                characteristic.uuid != APP_CHARACTERISTIC_ALT_UUID
            ) {
                return
            }
            val pending = pendingWrite ?: return
            pending.status = status
            pending.done.countDown()
        }

        override fun onCharacteristicChanged(
            currentGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
        ) {
            if (characteristic.uuid == APP_CHARACTERISTIC_UUID ||
                characteristic.uuid == APP_CHARACTERISTIC_ALT_UUID
            ) {
                frames.push(value)
            }
        }

        @Deprecated("Deprecated in API 33; the value overload above is used there")
        @Suppress("DEPRECATION")
        override fun onCharacteristicChanged(
            currentGatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
        ) {
            if (characteristic.uuid == APP_CHARACTERISTIC_UUID ||
                characteristic.uuid == APP_CHARACTERISTIC_ALT_UUID
            ) {
                characteristic.value?.let { frames.push(it) }
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
        private const val FRAME_QUEUE_CAPACITY = FrameQueue.DEFAULT_CAPACITY
        private const val CONNECT_TIMEOUT_MS = 10_000L
        private const val DISCOVERY_TIMEOUT_MS = 10_000L
        private const val CCCD_TIMEOUT_MS = 3_000L
        private const val MTU_TIMEOUT_MS = 3_000L
        private const val WRITE_TIMEOUT_MS = 2_000L
        private const val WRITE_ATTEMPTS = 100
        private const val WRITE_RETRY_DELAY_MS = 10L
    }
}
