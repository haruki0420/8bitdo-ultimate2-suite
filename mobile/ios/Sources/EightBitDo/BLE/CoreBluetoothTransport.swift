import CoreBluetooth
import EightBitdoFFI
import Foundation

/// CoreBluetooth implementation of the Rust `NativeTransport` callback.
///
/// # Responsibility split
///
/// `AppModel` owns the `CBCentralManager` connection lifecycle. This type only
/// mirrors the state of one already-connected link:
///
/// * it never starts, stops or retries a connection;
/// * when the central reports a disconnection, `AppModel` calls
///   `disconnect()` so [`recv`] fails immediately instead of waiting out its
///   timeout (the Rust bridge then closes its receive queue);
/// * `disconnect()` is idempotent and safe to call from any queue.
///
/// Rust calls `recv` from its own receive thread and `send` from the session
/// thread, while CoreBluetooth delivers delegate callbacks on the central
/// manager's queue. Frames therefore cross the boundary through an
/// `NSCondition`-protected bounded queue (oldest frame dropped, matching the
/// Rust-side `FrameQueue`) instead of any shared mutable state.
///
/// The MTU is not negotiable from iOS; `CBPeripheral.maximumWriteValueLength`
/// is the only public lower bound, so the transport reports
/// `maximumWriteValueLength(for: .withoutResponse) + 3` (the 3-byte ATT
/// notification header the Rust sizing code subtracts).
final class CoreBluetoothTransport: NSObject, NativeTransport, @unchecked Sendable {
    /// Same bound as the Rust bridge queue: enough for one full config read.
    static let frameCapacity = 64

    private let peripheral: CBPeripheral
    private let characteristic: CBCharacteristic
    private let callbackQueue: DispatchQueue
    private let mtuValue: UInt16
    private let supportsWriteWithoutResponse: Bool

    private let condition = NSCondition()
    private var frames: [Data] = []
    private var droppedFrames: UInt64 = 0
    private var closed = false

    init(
        peripheral: CBPeripheral,
        characteristic: CBCharacteristic,
        mtu: UInt16,
        callbackQueue: DispatchQueue
    ) {
        self.peripheral = peripheral
        self.characteristic = characteristic
        self.mtuValue = mtu
        self.callbackQueue = callbackQueue
        // Checked again in `send`, because a silent fallback to a write with
        // response (or a dropped write) would corrupt the frame stream.
        self.supportsWriteWithoutResponse =
            characteristic.properties.contains(.writeWithoutResponse)
        super.init()
        peripheral.delegate = self
    }

    // MARK: - NativeTransport

    func send(data: Data) throws {
        condition.lock()
        let isClosed = closed
        condition.unlock()
        guard !isClosed else {
            throw NativeTransportError.Failure(message: "BLE link is closed")
        }
        guard supportsWriteWithoutResponse else {
            throw NativeTransportError.Failure(
                message: "App Mode characteristic has no write-without-response property"
            )
        }
        // With .withoutResponse the stack drops a write when its buffer is
        // full. Failing closed (instead of writing anyway) keeps the frame
        // stream ordered; the Rust layer reports the read as failed.
        guard waitForWriteCapacity() else {
            throw NativeTransportError.Failure(
                message: "BLE write capacity did not free up within the timeout"
            )
        }

        callbackQueue.async { [peripheral, characteristic] in
            peripheral.writeValue(data, for: characteristic, type: .withoutResponse)
        }
    }

    func recv(timeoutMs: UInt32) throws -> Data? {
        let deadline = Date(timeIntervalSinceNow: Double(timeoutMs) / 1_000.0)
        condition.lock()
        defer { condition.unlock() }

        while frames.isEmpty {
            if closed {
                throw NativeTransportError.Failure(message: "BLE link is closed")
            }
            if !condition.wait(until: deadline) {
                return nil
            }
        }
        return frames.removeFirst()
    }

    func mtu() -> UInt16 {
        mtuValue
    }

    func disconnect() {
        condition.lock()
        closed = true
        frames.removeAll()
        condition.broadcast()
        condition.unlock()
    }

    // MARK: - Internal

    /// Wait until the stack accepts another write-without-response.
    ///
    /// Returns `false` when the wait timed out or the link closed.
    private func waitForWriteCapacity() -> Bool {
        condition.lock()
        defer { condition.unlock() }
        let deadline = Date(timeIntervalSinceNow: 2.0)
        while !peripheral.canSendWriteWithoutResponse && !closed {
            if !condition.wait(until: deadline) {
                return false
            }
        }
        return !closed
    }

    private func enqueue(_ data: Data) {
        condition.lock()
        if !closed {
            if frames.count >= Self.frameCapacity {
                frames.removeFirst()
                droppedFrames += 1
            }
            frames.append(data)
        }
        condition.signal()
        condition.unlock()
    }
}

extension CoreBluetoothTransport: CBPeripheralDelegate {
    func peripheral(
        _ peripheral: CBPeripheral,
        didUpdateValueFor characteristic: CBCharacteristic,
        error: Error?
    ) {
        guard error == nil, let value = characteristic.value else { return }
        enqueue(value)
    }

    func peripheralIsReady(toSendWriteWithoutResponse peripheral: CBPeripheral) {
        condition.lock()
        condition.broadcast()
        condition.unlock()
    }
}
