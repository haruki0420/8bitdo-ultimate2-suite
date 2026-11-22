//! `uniffi-bindgen` entry point (Kotlin / Swift from a built library).
//!
//! Run with the `cli` feature:
//!
//! ```text
//! cargo run -p eightbitdo-uniffi --features cli --bin uniffi-bindgen -- \
//!   generate --library target/debug/libeightbitdo_uniffi.so --language kotlin --out-dir <dir>
//! ```

fn main() {
    uniffi::uniffi_bindgen_main();
}
