package dev.eightbitdo.mobile.ble

import android.bluetooth.BluetoothGattCharacteristic

/**
 * How command frames are written to the App Mode characteristic.
 *
 * The protocol is normally driven with write-without-response, but some
 * FF11/FF13 implementations only advertise write-with-response. Both work with
 * the same framing; the choice only changes GATT acknowledgement behaviour.
 * A characteristic that advertises neither is refused at connect time.
 */
enum class AppWriteMode(val gattWriteType: Int) {
    /** No ATT response; fastest and the protocol's normal mode. */
    WITHOUT_RESPONSE(BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE),

    /** ATT write response is awaited before the next frame. */
    WITH_RESPONSE(BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT);

    companion object {
        /**
         * Pick the write type from a characteristic's `properties`.
         *
         * Prefers write-without-response, falls back to write-with-response,
         * and returns `null` when the characteristic is not writable at all.
         */
        fun select(properties: Int): AppWriteMode? = when {
            properties and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0 ->
                WITHOUT_RESPONSE

            properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0 ->
                WITH_RESPONSE

            else -> null
        }
    }
}
