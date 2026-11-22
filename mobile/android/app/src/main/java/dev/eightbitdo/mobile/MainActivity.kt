package dev.eightbitdo.mobile

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.lifecycle.ViewModelProvider
import dev.eightbitdo.mobile.ui.EightBitDoTheme
import dev.eightbitdo.mobile.ui.HomeScreen

class MainActivity : ComponentActivity() {

    private lateinit var deviceViewModel: DeviceViewModel

    private val requestBlePermissions =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { grants ->
            if (grants.values.all { it }) {
                deviceViewModel.startScan()
            }
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        deviceViewModel = ViewModelProvider(this)[DeviceViewModel::class.java]
        setContent {
            EightBitDoTheme {
                HomeScreen(deviceViewModel)
            }
        }
        if (BlePermissions.granted(this)) {
            deviceViewModel.startScan()
        } else {
            requestBlePermissions.launch(BlePermissions.required())
        }
    }
}
