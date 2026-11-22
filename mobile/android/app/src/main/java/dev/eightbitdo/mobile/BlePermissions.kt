package dev.eightbitdo.mobile

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat

/** Runtime BLE permissions for the current Android version. */
object BlePermissions {
    /** Permissions that must be granted before scanning or connecting. */
    fun required(): Array<String> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_CONNECT,
            )
        } else {
            // Android 11 and lower require location for BLE scanning.
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }

    /** `true` when every required permission is already granted. */
    fun granted(context: Context): Boolean = required().all { permission ->
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
    }
}
