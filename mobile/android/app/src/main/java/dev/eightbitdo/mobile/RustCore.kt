package dev.eightbitdo.mobile

import dev.eightbitdo.mobile.core.minimumReadMtu

/**
 * Detects whether the Rust UniFFI shared library is present before any
 * Bluetooth work starts.
 *
 * A Kotlin-only debug APK (built with `--kotlin-only`, without the NDK) has no
 * `libeightbitdo_uniffi.so`. The generated Kotlin binding calls
 * `System.loadLibrary` on first use, which throws `UnsatisfiedLinkError` — an
 * `Error`, not an `Exception` — so a naive `catch (e: Exception)` lets the
 * process crash. This object turns that into a UI message instead.
 */
object RustCore {
    enum class Availability {
        AVAILABLE,
        MISSING_LIBRARY,
        BROKEN,
    }

    /**
     * Try to touch the JNI library through a tiny exported function.
     *
     * Never throws: an `UnsatisfiedLinkError` (directly or wrapped in an
     * `ExceptionInInitializerError`) is reported as [Availability.MISSING_LIBRARY].
     */
    fun availability(): Availability = try {
        minimumReadMtu()
        Availability.AVAILABLE
    } catch (error: Throwable) {
        if (hasCause<UnsatisfiedLinkError>(error)) {
            Availability.MISSING_LIBRARY
        } else {
            Availability.BROKEN
        }
    }

    /** User-facing explanation; `null` when the core is usable. */
    fun message(availability: Availability): String? = when (availability) {
        Availability.AVAILABLE -> null
        Availability.MISSING_LIBRARY ->
            "この APK には Rust コア（libeightbitdo_uniffi.so）が含まれていません。" +
                "NDK ビルド版の APK（scripts/mobile/build-android.sh）が必要です。"
        Availability.BROKEN ->
            "Rust コアの初期化に失敗しました。アプリを再インストールしてください。"
    }

    private inline fun <reified T : Throwable> hasCause(error: Throwable): Boolean {
        var current: Throwable? = error
        while (current != null) {
            if (current is T) return true
            current = current.cause
        }
        return false
    }
}
