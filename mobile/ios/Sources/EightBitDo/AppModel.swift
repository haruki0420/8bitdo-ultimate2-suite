import Combine
import CoreBluetooth
import EightBitdoFFI
import Foundation

/// One controller seen during a scan.
struct DiscoveredDevice: Identifiable, Equatable {
    let id: UUID
    let name: String
    let rssi: Int
}

/// Owns the CoreBluetooth central and the single Rust session.
///
/// All CoreBluetooth work happens on `bleQueue`. Rust calls (`readDeviceInfo`,
/// `readConfig`, ...) block, so they run on a global queue; results are
/// published on the main thread. The Rust side serializes calls internally,
/// but the UI still avoids starting two reads at once.
final class AppModel: NSObject, ObservableObject {
    /// Coarse connection state shown in the UI.
    enum Stage: Equatable {
        case idle
        case unavailable(String)
        case scanning
        case connecting(String)
        case connected(String)
        case failed(String)

        var label: String {
            switch self {
            case .idle: return "Not connected"
            case .unavailable(let reason): return "Bluetooth unavailable: \(reason)"
            case .scanning: return "Scanning for controllers…"
            case .connecting(let name): return "Connecting to \(name)…"
            case .connected(let name): return "Connected to \(name)"
            case .failed(let reason): return "Failed: \(reason)"
            }
        }

        var isConnected: Bool {
            if case .connected = self { return true }
            return false
        }

        var isBusy: Bool {
            switch self {
            case .scanning, .connecting: return true
            default: return false
            }
        }
    }

    @Published private(set) var stage: Stage = .idle
    @Published private(set) var devices: [DiscoveredDevice] = []
    @Published private(set) var deviceInfo: DeviceInfo?
    @Published private(set) var configuration: DeviceConfig?
    @Published private(set) var macroBackup: MacroBackupView?
    @Published private(set) var isReading = false
    @Published private(set) var lastError: String?

    /// App Mode service / characteristic UUIDs from `docs/PROTOCOL_SPEC.md`.
    private static let serviceUUID = CBUUID(string: "0000FF10-0000-1000-8000-00805F9B34FB")
    private static let characteristicUUID = CBUUID(string: "0000FF11-0000-1000-8000-00805F9B34FB")
    private static let alternateCharacteristicUUID = CBUUID(
        string: "0000FF13-0000-1000-8000-00805F9B34FB"
    )
    private static let advertisedNamePrefix = "80ND"

    private let bleQueue = DispatchQueue(label: "dev.eightbitdo.mobile.ble")
    private var central: CBCentralManager?
    private var discoveredPeripherals: [UUID: CBPeripheral] = [:]
    private var activePeripheral: CBPeripheral?
    private var session: MobileSession?
    private var transport: CoreBluetoothTransport?
    private var wantsScan = false
    private var didSubscribe = false

    // MARK: - Scan / connect

    func startScan() {
        wantsScan = true
        lastError = nil
        devices = []
        if central == nil {
            central = CBCentralManager(
                delegate: self,
                queue: bleQueue,
                options: [CBCentralManagerOptionShowPowerAlertKey: true]
            )
        }
        bleQueue.async { [weak self] in
            guard let self else { return }
            guard let central = self.central, central.state == .poweredOn else { return }
            self.beginScanning()
        }
    }

    func stopScan() {
        wantsScan = false
        bleQueue.async { [weak self] in
            self?.central?.stopScan()
        }
    }

    func connect(_ device: DiscoveredDevice) {
        bleQueue.async { [weak self] in
            guard let self, let peripheral = self.discoveredPeripherals[device.id] else { return }
            self.wantsScan = false
            self.central?.stopScan()
            self.activePeripheral = peripheral
            peripheral.delegate = self
            self.publish { self.stage = .connecting(device.name) }
            self.central?.connect(peripheral, options: nil)
        }
    }

    func disconnect() {
        wantsScan = false
        didSubscribe = false
        let session = self.session
        self.session = nil
        let transport = self.transport
        self.transport = nil
        let peripheral = activePeripheral
        activePeripheral = nil

        publish {
            self.stage = .idle
            self.deviceInfo = nil
            self.configuration = nil
            self.macroBackup = nil
        }
        // Shut the Rust side down off the main thread: it joins the receive
        // worker (bounded by the transport poll interval).
        DispatchQueue.global(qos: .userInitiated).async {
            session?.disconnect()
            transport?.disconnect()
        }
        if let peripheral {
            bleQueue.async { [weak self] in
                self?.central?.cancelPeripheralConnection(peripheral)
            }
        }
    }

    // MARK: - Reads

    /// Read firmware version and the full configuration, one after the other.
    func refreshAll() {
        guard let session else { return }
        publish { self.isReading = true }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            do {
                let info = try session.readDeviceInfo()
                self.publish { self.deviceInfo = info }
                let config = try session.readConfig()
                self.publish { self.configuration = config }
            } catch {
                self.report(error)
            }
            self.publish { self.isReading = false }
        }
    }

    func refreshConfiguration() {
        guard let session, !isReading else { return }
        publish { self.isReading = true }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            do {
                let config = try session.readConfig()
                self.publish { self.configuration = config }
            } catch {
                self.report(error)
            }
            self.publish { self.isReading = false }
        }
    }

    func readMacroBackup() {
        guard let session, !isReading else { return }
        publish { self.isReading = true }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self else { return }
            do {
                let backup = try session.readMacroBackup()
                self.publish { self.macroBackup = backup }
            } catch {
                self.report(error)
            }
            self.publish { self.isReading = false }
        }
    }

    // MARK: - Helpers

    private func beginScanning() {
        guard let central, central.state == .poweredOn else { return }
        publish { self.stage = .scanning }
        // Unfiltered so a missing service UUID in the advertisement still
        // matches by advertised name; iOS only allows one scan at a time.
        central.scanForPeripherals(
            withServices: nil,
            options: [CBCentralManagerScanOptionAllowDuplicatesKey: false]
        )
    }

    private func publish(_ update: @escaping () -> Void) {
        if Thread.isMainThread {
            update()
        } else {
            DispatchQueue.main.async(execute: update)
        }
    }

    private func report(_ error: Error) {
        let message = (error as? LocalizedError)?.errorDescription ?? String(describing: error)
        publish {
            self.lastError = message
            if case .connected = self.stage {
                // Keep the connection state; the read failed but the link may
                // still be alive. The UI shows the error and offers a retry.
            }
        }
    }

    private func addOrUpdateDevice(_ peripheral: CBPeripheral, rssi: Int) {
        let name = peripheral.name ?? "Unnamed controller"
        let device = DiscoveredDevice(id: peripheral.identifier, name: name, rssi: rssi)
        discoveredPeripherals[peripheral.identifier] = peripheral
        publish {
            if let index = self.devices.firstIndex(where: { $0.id == device.id }) {
                self.devices[index] = device
            } else {
                self.devices.append(device)
            }
        }
    }

    private func attachSession(peripheral: CBPeripheral, characteristic: CBCharacteristic) {
        let writeLength = peripheral.maximumWriteValueLength(for: .withoutResponse)
        let mtu = UInt16(max(23, writeLength + 3))
        let transport = CoreBluetoothTransport(
            peripheral: peripheral,
            characteristic: characteristic,
            mtu: mtu,
            callbackQueue: bleQueue
        )
        do {
            let session = try MobileSession(transport: transport)
            self.transport = transport
            self.session = session
            let name = peripheral.name ?? "controller"
            publish { self.stage = .connected(name) }
            refreshAll()
        } catch {
            publish { self.stage = .failed("could not start the Rust session") }
            report(error)
        }
    }
}

// MARK: - CBCentralManagerDelegate

extension AppModel: CBCentralManagerDelegate {
    func centralManagerDidUpdateState(_ central: CBCentralManager) {
        switch central.state {
        case .poweredOn:
            if wantsScan {
                beginScanning()
            }
        case .unauthorized:
            publish { self.stage = .unavailable("Bluetooth permission denied") }
        case .poweredOff:
            publish { self.stage = .unavailable("Bluetooth is off") }
        case .unsupported:
            publish { self.stage = .unavailable("this device has no BLE support") }
        default:
            break
        }
    }

    func centralManager(
        _ central: CBCentralManager,
        didDiscover peripheral: CBPeripheral,
        advertisementData: [String: Any],
        rssi RSSI: NSNumber
    ) {
        let advertisedName = advertisementData[CBAdvertisementDataLocalNameKey] as? String
        let name = advertisedName ?? peripheral.name
        let advertisedServices =
            advertisementData[CBAdvertisementDataServiceUUIDsKey] as? [CBUUID] ?? []
        let matchesName = name?.hasPrefix(Self.advertisedNamePrefix) ?? false
        let matchesService = advertisedServices.contains(Self.serviceUUID)
        guard matchesName || matchesService else { return }
        addOrUpdateDevice(peripheral, rssi: RSSI.intValue)
    }

    func centralManager(_ central: CBCentralManager, didConnect peripheral: CBPeripheral) {
        peripheral.delegate = self
        peripheral.discoverServices([Self.serviceUUID])
    }

    func centralManager(
        _ central: CBCentralManager,
        didFailToConnect peripheral: CBPeripheral,
        error: Error?
    ) {
        let reason = error?.localizedDescription ?? "unknown reason"
        publish { self.stage = .failed("connection failed: \(reason)") }
    }

    func centralManager(
        _ central: CBCentralManager,
        didDisconnectPeripheral peripheral: CBPeripheral,
        error: Error?
    ) {
        session = nil
        transport = nil
        didSubscribe = false
        activePeripheral = nil
        let reason = error?.localizedDescription
        publish {
            self.stage = .idle
            self.deviceInfo = nil
            self.configuration = nil
            self.macroBackup = nil
            if let reason {
                self.lastError = "disconnected: \(reason)"
            }
        }
    }
}

// MARK: - CBPeripheralDelegate

extension AppModel: CBPeripheralDelegate {
    func peripheral(_ peripheral: CBPeripheral, didDiscoverServices error: Error?) {
        if let error {
            report(error)
            return
        }
        guard let service = peripheral.services?.first(where: { $0.uuid == Self.serviceUUID })
        else {
            publish { self.stage = .failed("App Mode service FF10 not found") }
            return
        }
        peripheral.discoverCharacteristics(
            [Self.characteristicUUID, Self.alternateCharacteristicUUID],
            for: service
        )
    }

    func peripheral(
        _ peripheral: CBPeripheral,
        didDiscoverCharacteristicsFor service: CBService,
        error: Error?
    ) {
        if let error {
            report(error)
            return
        }
        let characteristics = service.characteristics ?? []
        // FF11 is the documented App Mode characteristic; FF13 exists on some
        // units and speaks the same protocol.
        let characteristic =
            characteristics.first(where: { $0.uuid == Self.characteristicUUID })
            ?? characteristics.first(where: { $0.uuid == Self.alternateCharacteristicUUID })
        guard let characteristic else {
            publish { self.stage = .failed("App Mode characteristic FF11/FF13 not found") }
            return
        }
        guard characteristic.properties.contains(.notify) else {
            publish { self.stage = .failed("App Mode characteristic cannot notify") }
            return
        }
        peripheral.setNotifyValue(true, for: characteristic)
    }

    func peripheral(
        _ peripheral: CBPeripheral,
        didUpdateNotificationStateFor characteristic: CBCharacteristic,
        error: Error?
    ) {
        if let error {
            report(error)
            return
        }
        guard characteristic.isNotifying, !didSubscribe else { return }
        didSubscribe = true
        attachSession(peripheral: peripheral, characteristic: characteristic)
    }
}
