package com.rusticmaps.sdk.refapprn

import android.os.Bundle
import com.facebook.react.ReactActivity
import com.facebook.react.ReactActivityDelegate
import com.facebook.react.defaults.DefaultNewArchitectureEntryPoint.fabricEnabled
import com.facebook.react.defaults.DefaultReactActivityDelegate

class MainActivity : ReactActivity() {

  /**
   * Returns the name of the main component registered from JavaScript. This is used to schedule
   * rendering of the component.
   */
  override fun getMainComponentName(): String = "MapsSdkRefappRn"

  /**
   * Returns the instance of the [ReactActivityDelegate]. We use [DefaultReactActivityDelegate]
   * which allows you to enable New Architecture with a single boolean flags [fabricEnabled]
   */
  override fun createReactActivityDelegate(): ReactActivityDelegate =
      DefaultReactActivityDelegate(this, mainComponentName, fabricEnabled)

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    // Software-renderer environment workaround (documented, demo only), the
    // Compose reference application's identical line: this AVD's graphics stack
    // is ANGLE over SwiftShader Vulkan, whose swapchains advertise only the
    // `Inherit` composite-alpha mode, which the SDK's `Opaque` surface request
    // rejects straight out. wgpu selects the backend at device creation, so the
    // GL (EGL/GLES) backend is requested before the first SDK call; on hardware
    // this line is a no-op for the default Vulkan selection only inside this
    // demo.
    android.system.Os.setenv("WGPU_BACKEND", "gl", false)
  }
}
