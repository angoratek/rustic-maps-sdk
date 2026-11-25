//! The Android JNI seam of the shipped [`MapView`](crate::surface_view::MapView):
//! the `SurfaceHandle` record needs an `ANativeWindow*`,
//! and Android exposes no public Kotlin API for one. This module exports the
//! two fixed JNI symbols the SDK's Kotlin `NativeWindowBridge` object declares,
//! compiled into the same `librustic_maps.so` the UniFFI bindings load, so
//! an integrator ships no C and no pointer arithmetic of their own.
//!
//! The bound Kotlin class and package are a compile-time constant of the
//! mangled symbol below: `com.rusticmaps.sdk.NativeWindowBridge`. Renaming
//! or moving that object silently breaks the link, which is why the wrapper
//! module ships a consumer ProGuard keep rule for it.
//!
//! The acquisition and release pair follows `ANativeWindow_fromSurface`'s
//! reference-counted ownership: every acquired pointer must be released
//! exactly once, and the Kotlin wrapper releases it on every failure path
//! (a failed `attach` included) instead of leaking one reference per attempt.
//! The pointer is opaque on the Kotlin side; it is never dereferenced there.

use jni_sys::{JNIEnv, jlong, jobject};
use ndk_sys::{ANativeWindow, ANativeWindow_fromSurface, ANativeWindow_release};

/// The zero pointer value the JVM sees when no window could be acquired.
///
/// Kotlin lowers this to `null` for the SDK's exact-one-platform-pointer
/// validation, so the failure surfaces as the contract's
/// `InvalidRenderInput` rather than as a zeroed handle.
const NO_NATIVE_WINDOW_POINTER: jlong = 0;

/// Acquires the `ANativeWindow` behind a platform `android.view.Surface`.
///
/// Bound to `com.rusticmaps.sdk.NativeWindowBridge.nativeWindowPointerFromSurface`.
/// Answers the raw `ANativeWindow*` as a `jlong`, or zero when the surface is
/// null or the platform refuses the acquisition.
///
/// # Safety
///
/// The JVM calls this only through the matching `external fun`; both pointer
/// arguments are JNI-owned for the duration of the call.
// The function must stay a safe `extern "system"` symbol for the JVM's symbol
// lookup; the pointer contract above is the JNI caller's responsibility, so
// the lint's usual "mark it unsafe" advice does not apply to an exported JNI
// entry point.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rusticmaps_sdk_NativeWindowBridge_nativeWindowPointerFromSurface(
    environment: *mut JNIEnv,
    _receiver: jobject,
    surface: jobject,
) -> jlong {
    if surface.is_null() {
        return NO_NATIVE_WINDOW_POINTER;
    }
    // SAFETY: the JVM guarantees a live `JNIEnv*` and a live local `Surface`
    // reference for the duration of the native call; `ANativeWindow_fromSurface`
    // answers an owned window reference (or null) that the Kotlin wrapper
    // releases through the paired symbol below.
    let native_window: *mut ANativeWindow =
        unsafe { ANativeWindow_fromSurface(environment, surface) };
    native_window as jlong
}

/// Releases a window acquired by
/// [`Java_com_rusticmaps_sdk_NativeWindowBridge_nativeWindowPointerFromSurface`].
///
/// Bound to `com.rusticmaps.sdk.NativeWindowBridge.releaseNativeWindow`.
/// A zero pointer is a no-op, so the Kotlin wrapper can release
/// unconditionally on teardown paths.
///
/// # Safety
///
/// `native_window_pointer` must be a pointer previously answered by the
/// acquisition symbol and not released since; the JVM calls this only through
/// the matching `external fun`.
// The same exported-JNI-entry-point exception as the acquisition symbol.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_rusticmaps_sdk_NativeWindowBridge_releaseNativeWindow(
    _environment: *mut JNIEnv,
    _receiver: jobject,
    native_window_pointer: jlong,
) {
    if native_window_pointer == NO_NATIVE_WINDOW_POINTER {
        return;
    }
    // SAFETY: the non-zero pointer is the acquisition symbol's owned window
    // reference; releasing it exactly once is the paired contract above.
    unsafe { ANativeWindow_release(native_window_pointer as *mut ANativeWindow) };
}
