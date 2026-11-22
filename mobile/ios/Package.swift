// swift-tools-version: 6.0
import Foundation
import PackageDescription

// xtool generates its builder package in `xtool/.xtool-tmp` and links the app
// there, so a relative `-L` would resolve against that directory. Derive the
// absolute path of this manifest instead; `scripts/mobile/build-ios.sh` puts
// the Rust static library in `libs/`.
let packageDirectory = URL(fileURLWithPath: #filePath).deletingLastPathComponent().path
let rustLibsDirectory = "\(packageDirectory)/libs"

let package = Package(
    name: "EightBitDo",
    platforms: [
        // Matches the deployment target the UniFFI scaffolding and CoreBluetooth
        // usage were checked against.
        .iOS(.v17),
    ],
    products: [
        // An xtool project must contain exactly one library product: the app.
        .library(
            name: "EightBitDo",
            targets: ["EightBitDo"]
        ),
    ],
    targets: [
        // Clang module for the UniFFI C ABI. `RustCore/` holds the generated
        // `module.modulemap` and `EightBitdoFFI.h` (not committed; run
        // `scripts/mobile/gen-bindings.sh`).
        .systemLibrary(
            name: "EightBitdoFFI",
            path: "RustCore"
        ),
        .target(
            name: "EightBitDo",
            dependencies: ["EightBitdoFFI"],
            linkerSettings: [
                .unsafeFlags(["-L", rustLibsDirectory]),
                .linkedLibrary("eightbitdo_uniffi"),
            ]
        ),
    ],
    swiftLanguageModes: [.v5]
)
