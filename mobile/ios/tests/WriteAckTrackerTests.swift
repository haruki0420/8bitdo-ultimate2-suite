// Standalone checks for WriteAckTracker.
//
// Compiled and run by `scripts/mobile/test-write-ack-tracker.sh` on any Swift
// host (no XCTest, no CoreBluetooth): the tracker is pure logic.
//
// `@main` is used so `swiftc WriteAckTracker.swift WriteAckTrackerTests.swift`
// produces a runnable binary.

@main
struct WriteAckTrackerTests {
    static func main() {
        normalCompletion()
        timeoutRejectsLateCallback()
        timeoutBlocksFurtherWrites()
        concurrentWriteIsRefused()
        resetOnDisconnect()
        print("WriteAckTrackerTests: all checks passed")
    }

    private static func expect(_ condition: Bool, _ message: @autoclosure () -> String) {
        if !condition {
            fatalError("FAIL: \(message())")
        }
    }

    private static func normalCompletion() {
        var tracker = WriteAckTracker()
        guard let first = tracker.begin() else {
            expect(false, "begin() must succeed on an idle tracker")
            return
        }
        expect(tracker.isPending(first), "the first generation must be pending")
        expect(tracker.completeCurrent() == first, "the callback must complete its generation")
        expect(!tracker.isPending(first), "a completed write must not stay pending")
        guard let second = tracker.begin() else {
            expect(false, "begin() must succeed after completion")
            return
        }
        expect(second == first + 1, "generations must increase monotonically")
        expect(tracker.completeCurrent() == second, "the second generation must complete")
        expect(!tracker.needsReconnect, "a successful write must not require a reconnect")
    }

    private static func timeoutRejectsLateCallback() {
        var tracker = WriteAckTracker()
        guard let generation = tracker.begin() else {
            expect(false, "begin() must succeed")
            return
        }
        expect(tracker.timeOut(generation), "timeOut must accept the pending generation")
        expect(tracker.needsReconnect, "a timeout must mark the link for reconnect")
        expect(!tracker.isPending(generation), "a timed-out write is no longer pending")
        // A callback that arrives after the timeout must be dropped, not
        // attributed to whatever comes next.
        expect(tracker.completeCurrent() == nil, "a late callback must be ignored")
    }

    private static func timeoutBlocksFurtherWrites() {
        var tracker = WriteAckTracker()
        guard let generation = tracker.begin() else {
            expect(false, "begin() must succeed")
            return
        }
        _ = tracker.timeOut(generation)
        expect(tracker.begin() == nil, "begin() must refuse after a lost write response")
        expect(!tracker.timeOut(generation), "a second timeout for the same generation is a no-op")
    }

    private static func concurrentWriteIsRefused() {
        var tracker = WriteAckTracker()
        guard let generation = tracker.begin() else {
            expect(false, "begin() must succeed")
            return
        }
        expect(tracker.begin() == nil, "a second in-flight write must be refused")
        expect(tracker.isPending(generation), "the first write stays pending")
        expect(tracker.completeCurrent() == generation, "it must still complete")
    }

    private static func resetOnDisconnect() {
        var tracker = WriteAckTracker()
        guard tracker.begin() != nil else {
            expect(false, "begin() must succeed")
            return
        }
        tracker.reset()
        expect(tracker.pending == nil, "reset must drop the pending generation")
        expect(tracker.begin() != nil, "a fresh link may begin again")
    }
}
