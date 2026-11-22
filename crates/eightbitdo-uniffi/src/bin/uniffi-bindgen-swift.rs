//! `uniffi-bindgen-swift` entry point.
//!
//! This generator writes a Swift source file, a C header and a modulemap that
//! the xtool / SwiftPM project consumes as a `.systemLibrary`. Run it with the
//! `cli` feature (see `scripts/mobile/gen-bindings.sh`).

fn main() {
    uniffi::uniffi_bindgen_swift();
}
