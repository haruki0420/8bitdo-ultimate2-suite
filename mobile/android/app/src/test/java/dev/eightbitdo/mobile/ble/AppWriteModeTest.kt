package dev.eightbitdo.mobile.ble

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AppWriteModeTest {

    // BluetoothGattCharacteristic constants are compile-time ints:
    // PROPERTY_WRITE_NO_RESPONSE = 0x04, PROPERTY_WRITE = 0x08,
    // PROPERTY_NOTIFY = 0x10.
    @Test
    fun `prefers write without response when both are available`() {
        assertEquals(AppWriteMode.WITHOUT_RESPONSE, AppWriteMode.select(0x04 or 0x08))
    }

    @Test
    fun `falls back to write with response`() {
        assertEquals(AppWriteMode.WITH_RESPONSE, AppWriteMode.select(0x08))
    }

    @Test
    fun `returns null when the characteristic is not writable`() {
        assertNull(AppWriteMode.select(0x10))
        assertNull(AppWriteMode.select(0x00))
    }
}
