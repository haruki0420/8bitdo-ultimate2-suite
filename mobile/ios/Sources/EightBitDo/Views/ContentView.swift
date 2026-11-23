import SwiftUI

/// Top-level screen: connection, firmware info, configuration and macros.
///
/// The UI is intentionally read-only for now. Configuration writes need the
/// backup + confirmation flow that `eightbitdo-core` already supports; the
/// UniFFI surface exposes only write *preparation* until that flow lands.
struct ContentView: View {
    @EnvironmentObject private var model: AppModel

    var body: some View {
        NavigationStack {
            List {
                connectionSection
                if !model.devices.isEmpty {
                    devicesSection
                }
                if let info = model.deviceInfo {
                    firmwareSection(info)
                }
                if let config = model.configuration {
                    configurationSection(config)
                }
                macroSection
                if let error = model.lastError {
                    Section("Last error") {
                        Text(error)
                            .font(.callout)
                            .foregroundStyle(.red)
                            .textSelection(.enabled)
                    }
                }
            }
            .navigationTitle("Padbench")
            .toolbar {
                ToolbarItem(placement: .primaryAction) {
                    if model.stage.isConnected {
                        Button("Disconnect") { model.disconnect() }
                    } else {
                        Button("Scan") { model.startScan() }
                            .disabled(model.stage.isBusy)
                    }
                }
            }
            .refreshable {
                if model.stage.isConnected {
                    model.refreshAll()
                } else {
                    model.startScan()
                }
            }
        }
    }

    private var connectionSection: some View {
        Section("Connection") {
            HStack {
                Text(model.stage.label)
                Spacer()
                if model.stage.isBusy || model.isReading {
                    ProgressView()
                }
            }
            if model.stage.isConnected {
                Button("Re-read version and configuration") {
                    model.refreshAll()
                }
                .disabled(model.isReading)
            }
        }
    }

    private var devicesSection: some View {
        Section("Controllers") {
            ForEach(model.devices) { device in
                Button {
                    model.connect(device)
                } label: {
                    HStack {
                        Text(device.name)
                            .foregroundStyle(.primary)
                        Spacer()
                        Text("\(device.rssi) dBm")
                            .font(.caption.monospacedDigit())
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    private func firmwareSection(_ info: DeviceInfo) -> some View {
        Section("Firmware") {
            LabeledContent("Version", value: versionText(info))
            LabeledContent("Raw", value: info.raw.map { String(format: "%02X", $0) }.joined(separator: " "))
        }
    }

    private func versionText(_ info: DeviceInfo) -> String {
        if !info.text.isEmpty {
            return info.text
        }
        // The controller reports a little-endian i32; show it as decimal as
        // well because there is no printable version string.
        if info.raw.count == 4 {
            let value = info.raw.withUnsafeBytes { $0.load(as: UInt32.self) }
            return "\(value) (binary)"
        }
        return "—"
    }

    private func configurationSection(_ config: DeviceConfig) -> some View {
        Section("Configuration") {
            LabeledContent("CRC") {
                Text(
                    config.crcValid
                        ? String(format: "valid (0x%04X)", config.computedCrc)
                        : String(format: "MISMATCH stored 0x%04X, computed 0x%04X",
                                 config.storedCrc, config.computedCrc)
                )
                .foregroundStyle(config.crcValid ? Color.secondary : Color.red)
            }
            LabeledContent("Mode", value: config.summary.gamepadMode)
            LabeledContent("Active slot") {
                if let slot = config.summary.currentSlot {
                    Text("\(slot)")
                } else {
                    Text("—")
                }
            }
            ForEach(Array(config.summary.profileNames.enumerated()), id: \.offset) { index, name in
                LabeledContent("Profile \(index + 1)", value: name.isEmpty ? "—" : name)
            }
            LabeledContent(
                "Snapshot",
                value: "\(config.raw.count) bytes, 0x\(config.raw.prefix(4).map { String(format: "%02X", $0) }.joined())…"
            )
            Button("Re-read configuration") {
                model.refreshConfiguration()
            }
            .disabled(model.isReading)
        }
    }

    private var macroSection: some View {
        Section("Macros") {
            if let backup = model.macroBackup {
                ForEach(backup.entries, id: \.self) { entry in
                    LabeledContent(
                        "Slot \(entry.slot) · macro \(entry.index + 1)",
                        value: entry.isEmpty
                            ? "empty"
                            : "\(entry.name.isEmpty ? "unnamed" : entry.name) · \(entry.stepCount) steps"
                    )
                }
            } else {
                Text("Not read yet.")
                    .foregroundStyle(.secondary)
            }
            Button("Back up macros (3 slots × 4)") {
                model.readMacroBackup()
            }
            .disabled(!model.stage.isConnected || model.isReading)
        }
    }
}
