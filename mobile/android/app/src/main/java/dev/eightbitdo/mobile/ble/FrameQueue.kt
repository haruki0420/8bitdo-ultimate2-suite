package dev.eightbitdo.mobile.ble

/**
 * A bounded notification-frame queue with drop-oldest overflow handling.
 *
 * This mirrors the Rust-side `FrameQueue`: Android delivers notifications on a
 * Binder thread and the Rust bridge consumes them from its receive thread. The
 * controller can stream 20 ms input reports during a slow configuration read,
 * so an unbounded queue would grow without limit. When the queue is full the
 * **oldest** frame is dropped and counted; the affected Rust read then fails
 * closed (timeout / resync) instead of consuming a corrupt transfer.
 */
class FrameQueue(private val capacity: Int = DEFAULT_CAPACITY) {

    private val lock = Any()
    private val frames = ArrayDeque<ByteArray>()
    private var closed = false
    private var dropped: Long = 0

    init {
        require(capacity > 0) { "capacity must be positive" }
    }

    /** Push one frame, dropping the oldest when the queue is full. */
    fun push(frame: ByteArray) {
        synchronized(lock) {
            if (closed) return
            if (frames.size >= capacity) {
                frames.removeFirst()
                dropped++
            }
            frames.addLast(frame)
            lock.notifyAll()
        }
    }

    /**
     * Pop the oldest frame, or return `null` on timeout or after [close].
     *
     * Callers must distinguish a timeout from a close with [isClosed] and
     * surface the latter as a link error.
     */
    fun poll(timeoutMs: Long): ByteArray? {
        synchronized(lock) {
            val deadline = System.nanoTime() + timeoutMs * 1_000_000
            while (frames.isEmpty()) {
                if (closed) return null
                val remaining = deadline - System.nanoTime()
                if (remaining <= 0) return null
                val millis = remaining / 1_000_000
                lock.wait(if (millis > 0) millis else 1)
            }
            return frames.removeFirst()
        }
    }

    /** Close the queue: clear buffered frames and wake every waiter. */
    fun close() {
        synchronized(lock) {
            closed = true
            frames.clear()
            lock.notifyAll()
        }
    }

    /** Number of frames dropped because of overflow. */
    fun droppedFrames(): Long = synchronized(lock) { dropped }

    /** `true` once [close] was called. */
    fun isClosed(): Boolean = synchronized(lock) { closed }

    companion object {
        /** Same bound as the Rust bridge queue: enough for one full config read. */
        const val DEFAULT_CAPACITY = 64
    }
}
