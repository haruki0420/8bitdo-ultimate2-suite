// Top-level build file. Plugin versions live in gradle/libs.versions.toml.
// AGP 9 has built-in Kotlin support, so the org.jetbrains.kotlin.android
// plugin must not be applied (it fails to apply by design).
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.compose) apply false
}
