package dev.eightbitdo.mobile.ble

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FrameQueueTest {

    @Test
    fun `drops the oldest frames when full`() {
        val queue = FrameQueue(capacity = 3)
        for (index in 0..4) {
            queue.push(byteArrayOf(index.toByte()))
        }
        assertEquals(2, queue.droppedFrames())
        assertArrayEquals(byteArrayOf(2), queue.poll(0))
        assertArrayEquals(byteArrayOf(3), queue.poll(0))
        assertArrayEquals(byteArrayOf(4), queue.poll(0))
        assertNull(queue.poll(0))
    }

    @Test
    fun `close clears buffered frames`() {
        val queue = FrameQueue(capacity = 4)
        queue.push(byteArrayOf(7))
        queue.close()
        assertNull(queue.poll(0))
        assertTrue(queue.isClosed())
    }

    @Test
    fun `close wakes a blocked poll`() {
        val queue = FrameQueue(capacity = 4)
        val consumer = Thread {
            // Blocks until close() wakes it.
            assertNull(queue.poll(10_000))
            assertTrue(queue.isClosed())
        }
        consumer.start()
        Thread.sleep(50)
        queue.close()
        consumer.join(5_000)
        assertTrue(!consumer.isAlive)
    }
}
