package dev.eightbitdo.mobile.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

/** Minimal Material 3 theme; the brand polish lands in a later milestone. */
@Composable
fun EightBitDoTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = lightColorScheme(), content = content)
}
