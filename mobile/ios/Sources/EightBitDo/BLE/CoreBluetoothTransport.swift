import CoreBluetooth
import EightBitdoFFI
import Foundation

/// CoreBluetooth implementation of the Rust `NativeTransport` callback.
///
/// Rust calls `recv` from its own receive thread and `send` from the session
/// thread, while CoreBluetooth delivers delegate callbacks on the central
/// manager's queue. Frames therefore cross the boundary through an
/// `NSCondition`-protected queue instead of any shared mutable state.
///
/// The MTU is not negotiable from iOS; `CBPeripheral.maximumWriteValueLength`
/// is the only public lower bound, so the transport reports
/// `maximumWriteValueLength(for: .withoutResponse) + 3` (the 3-byte ATT
/// notification header the Rust sizing code subtracts).
final class CoreBluetoothTransport: NSObject, NativeTransport, @unchecked Sendable {
    private let peripheral: CBPeripheral
    private let characteristic: CBCharacteristic
    private let callbackQueue: DispatchQueue
    private let mtuValue: UInt16

    private let condition = NSCondition()
    private var frames: [Data] = []
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

        // With .withoutResponse the stack drops a write when its buffer is
        // full, so wait for `peripheralIsReady(toSendWriteWithoutResponse:)`
        // before each chunk. Bounded so a stuck controller cannot hang Rust.
        waitForWriteCapacity()

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
        condition.broadcast()
        condition.unlock()
    }

    // MARK: - Internal

    private func waitForWriteCapacity() {
        condition.lock()
        defer { condition.unlock() }
        let deadline = Date(timeIntervalSinceNow: 2.0)
        while !peripheral.canSendWriteWithoutResponse && !closed {
            if !condition.wait(until: deadline) {
                return
            }
        }
    }

    private func enqueue(_ data: Data) {
        condition.lock()
        frames.append(data)
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
