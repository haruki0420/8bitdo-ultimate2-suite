package dev.eightbitdo.mobile

import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class RustCoreTest {

    @Test
    fun `missing native library is reported instead of crashing`() {
        // The JVM unit test classpath has no libeightbitdo_uniffi.so, so this
        // exercises the same path as a Kotlin-only APK.
        assertNotEquals(RustCore.Availability.AVAILABLE, RustCore.availability())
    }

    @Test
    fun `missing library message names the NDK build`() {
        val message = RustCore.message(RustCore.Availability.MISSING_LIBRARY)
        assertTrue(message != null && message.contains("NDK"))
        assertNull(RustCore.message(RustCore.Availability.AVAILABLE))
    }
}
