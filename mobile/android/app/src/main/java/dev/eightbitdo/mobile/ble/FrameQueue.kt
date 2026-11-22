package dev.eightbitdo.mobile.ble

import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

/**
 * A bounded notification-frame queue with drop-oldest overflow handling.
 *
 * This mirrors the Rust-side `FrameQueue`: Android delivers notifications on a
 * Binder thread and the Rust bridge consumes them from its receive thread. The
 * controller can stream 20 ms input reports during a slow configuration read,
 * so an unbounded queue would grow without limit. When the queue is full the
 * **oldest** frame is dropped and counted; the affected Rust read then fails
 * closed (timeout / resync) instead of consuming a corrupt transfer.
 *
 * A dropped frame can be a read response. That is deliberate and safe today
 * because every affected read fails; when a flash-write path is wired in,
 * revisit this to drop only realtime input reports.
 */
class FrameQueue(private val capacity: Int = DEFAULT_CAPACITY) {

    private val lock = ReentrantLock()
    private val changed = lock.newCondition()
    private val frames = ArrayDeque<ByteArray>()
    private var closed = false
    private var dropped: Long = 0

    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    /** Push one frame, dropping the oldest when the queue is full. */
    fun push(frame: ByteArray) {
        lock.withLock {
            if (closed) return
            if (frames.size >= capacity) {
                frames.removeFirst()
                dropped++
            }
            frames.addLast(frame)
            changed.signalAll()
        }
    }

    /**
     * Pop the oldest frame, or return `null` on timeout or after [close].
     *
     * Callers must distinguish a timeout from a close with [isClosed] and
     * surface the latter as a link error.
     */
    fun poll(timeoutMs: Long): ByteArray? {
        lock.withLock {
            var remaining = TimeUnit.MILLISECONDS.toNanos(timeoutMs)
            while (frames.isEmpty()) {
                if (closed) return null
                if (remaining <= 0) return null
                remaining = changed.awaitNanos(remaining)
            }
            return frames.removeFirst()
        }
    }

    /** Close the queue: clear buffered frames and wake every waiter. */
    fun close() {
        lock.withLock {
            closed = true
            frames.clear()
            changed.signalAll()
        }
    }

    /** Number of frames dropped because of overflow. */
    fun droppedFrames(): Long = lock.withLock { dropped }

    /** `true` once [close] was called. */
    fun isClosed(): Boolean = lock.withLock { closed }

    companion object {
        /** Same bound as the Rust bridge queue: enough for one full config read. */
        const val DEFAULT_CAPACITY = 64
    }
}
