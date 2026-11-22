/// Tracks the single in-flight write-with-response ATT transaction.
///
/// CoreBluetooth's `didWriteValueFor` callback carries no request identifier,
/// so a callback that arrives after its write timed out would otherwise be
/// attributed to the next write. This state machine makes that misattribution
/// impossible:
///
/// * every write gets a monotonically increasing generation;
/// * `completeCurrent` only accepts a callback while a write is pending, so a
///   late callback after a timeout is dropped;
/// * a timeout permanently marks the link `needsReconnect`, so no new write can
///   be started and a late callback has no successor to be attributed to.
///
/// Pure value type with no CoreBluetooth dependency, so it can be exercised by
/// `scripts/mobile/test-write-ack-tracker.sh` on any Swift host.
struct WriteAckTracker: Equatable {
    /// Generation of the write that is currently waiting for a callback.
    private(set) var pending: UInt64?

    /// Generation that last received a callback.
    private(set) var lastCompleted: UInt64?

    /// `true` after a timeout: the link can no longer be used for
    /// write-with-response until it is reconnected.
    private(set) var needsReconnect = false

    private var nextGeneration: UInt64 = 1

    /// Start a write and return its generation.
    ///
    /// Returns `nil` when another write is still pending or when a previous
    /// write timed out (`needsReconnect`).
    mutating func begin() -> UInt64? {
        guard !needsReconnect, pending == nil else {
            return nil
        }
        let generation = nextGeneration
        nextGeneration += 1
        pending = generation
        return generation
    }

    /// Accept the in-flight write's callback.
    ///
    /// Returns the generation whose callback was accepted, or `nil` when no
    /// write is pending (a late callback from a timed-out write).
    mutating func completeCurrent() -> UInt64? {
        guard let generation = pending else {
            return nil
        }
        pending = nil
        lastCompleted = generation
        return generation
    }

    /// Mark `generation` as timed out.
    ///
    /// Returns `true` when it was still pending. The link is then marked
    /// `needsReconnect` and a late callback is ignored because no pipeline is
    /// waiting for it.
    @discardableResult
    mutating func timeOut(_ generation: UInt64) -> Bool {
        guard pending == generation else {
            return false
        }
        pending = nil
        needsReconnect = true
        return true
    }

    /// `true` when `generation` is the write still waiting for a callback.
    func isPending(_ generation: UInt64) -> Bool {
        pending == generation
    }

    /// Drop the pending generation on disconnect (keeps `needsReconnect`).
    mutating func reset() {
        pending = nil
    }
}
