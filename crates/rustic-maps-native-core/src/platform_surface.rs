//! The per-platform lowering of the exported [`crate::records::SurfaceHandle`]
//! into a `wgpu::SurfaceTargetUnsafe`: the seam rustic-maps-rendering's surface
//! renderer deliberately leaves open. Each supported platform compiles its
//! own guarded branch (macOS on the host, iOS and Android only under their
//! cross targets); every other target refuses at the boundary instead of
//! guessing which pointer to read.
//!
//! The record's exactly-one-pointer contract is validated once, portable:
//! a `u64 0` is a null pointer the lowering must never dereference, and so
//! is more than one pointer, so nothing invalid reaches the unsafe surface
//! creation. The single pointer is then re-checked for this platform's
//! field, so a handle built for another platform reads as a clean
//! validation failure rather than a wrong-pointer surface (and with it
//! undefined behavior of the kind `wgpu`'s unsafe surface contract rules
//! out).

// The imports and helpers below live only in the platform branches, so
// their cfg mirrors the branches' predicate: on a target with no branch the
// fallback never names any of them, and ungated code would read as unused.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
use std::ptr::NonNull;

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use crate::error::SdkError;
use crate::records::SurfaceHandle;

/// Count of platform pointers a valid surface handle carries: exactly one.
const VALID_HANDLE_POINTER_COUNT: usize = 1;

/// Lowers the handle onto the surface target of the compiling target's
/// platform. Fails for a handle whose shape is not exactly one non-null
/// pointer, whose single pointer is not this platform's, or when this
/// target has no platform implementation at all.
pub(crate) fn surface_target_from_handle(
    surface_handle: &SurfaceHandle,
) -> Result<wgpu::SurfaceTargetUnsafe, SdkError> {
    let set_platform_pointer = validated_single_pointer(surface_handle)?;
    this_platform_surface_target(surface_handle, set_platform_pointer)
}

/// Validates the handle's portable shape: exactly one platform pointer is
/// set and no set pointer is zero. Returns the platform field carrying the
/// set pointer, so the target lowering can check which pointer actually
/// won.
fn validated_single_pointer(
    surface_handle: &SurfaceHandle,
) -> Result<(PlatformPointerKind, u64), SdkError> {
    let set_pointers = [
        (
            PlatformPointerKind::AppKitView,
            surface_handle.appkit_view_pointer,
        ),
        (
            PlatformPointerKind::IosView,
            surface_handle.ios_view_pointer,
        ),
        (
            PlatformPointerKind::AndroidNativeWindow,
            surface_handle.android_native_window_pointer,
        ),
    ]
    .into_iter()
    .filter_map(|(pointer_kind, platform_pointer)| {
        platform_pointer.map(|platform_pointer_value| (pointer_kind, platform_pointer_value))
    })
    .collect::<Vec<_>>();
    if set_pointers.len() != VALID_HANDLE_POINTER_COUNT {
        return Err(SdkError::InvalidRenderInput {
            detail: format!(
                "the surface handle must carry exactly one platform pointer, \
                 not {}",
                set_pointers.len()
            ),
        });
    }
    let (pointer_kind, platform_pointer) = set_pointers[0];
    if platform_pointer == 0 {
        return Err(SdkError::InvalidRenderInput {
            detail: format!(
                "the surface handle's {} is zero; an actual platform object \
                 pointer is required",
                pointer_kind.render_input_name(),
            ),
        });
    }
    Ok((pointer_kind, platform_pointer))
}

/// Which platform pointer a handle carries, so the target lowering can
/// check the single set pointer belongs to its own platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlatformPointerKind {
    /// The macOS `NSView` pointer.
    AppKitView,
    /// The iOS `UIView` pointer.
    IosView,
    /// The Android `ANativeWindow` pointer.
    AndroidNativeWindow,
}

impl PlatformPointerKind {
    /// The name validation messages use for the pointer field, mirroring
    /// the record's field names.
    fn render_input_name(self) -> &'static str {
        match self {
            Self::AppKitView => "appkit view pointer",
            Self::IosView => "iOS view pointer",
            Self::AndroidNativeWindow => "Android native window pointer",
        }
    }
}

/// Refuses when the single set pointer is not this platform's pointer
/// field; otherwise the pointer is this platform's actual object pointer
/// and the lowering may turn it into a raw window handle.
///
/// Only the platform branches below call it, so the cfg mirrors theirs:
/// on a target with no branch the fallback lowers nothing and never reads
/// a pointer field at all.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
fn this_platform_pointer(platform_pointer: Option<u64>) -> Result<u64, SdkError> {
    platform_pointer.ok_or(SdkError::InvalidRenderInput {
        detail: "the surface handle's single pointer is not this platform's \
                 platform pointer; the handle was built for a different \
                 platform"
            .to_string(),
    })
}

/// Turns a validated non-zero object pointer into a raw handle's non-null
/// view/window pointer; the caller that owns the handle guarantees the
/// pointer stays valid (see the record's documentation).
///
/// Same cfg as [`this_platform_pointer`]: its only callers are the platform
/// branches.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
fn validated_platform_pointer(platform_pointer: u64) -> NonNull<core::ffi::c_void> {
    // The portable validation already refused the zero pointer; this
    // conversion turns the `u64` into the raw pointer type only. The
    // established invariant means the panicking helper is the honest form
    // (see the panic boundary's module documentation).
    crate::panic_boundary::required_invariant(
        NonNull::new(platform_pointer as *mut core::ffi::c_void),
        "the validated pointers are all non-zero",
    )
}

#[cfg(target_os = "macos")]
fn this_platform_surface_target(
    surface_handle: &SurfaceHandle,
    set_platform_pointer: (PlatformPointerKind, u64),
) -> Result<wgpu::SurfaceTargetUnsafe, SdkError> {
    use raw_window_handle::{AppKitDisplayHandle, AppKitWindowHandle};
    if set_platform_pointer.0 != PlatformPointerKind::AppKitView {
        return Err(SdkError::InvalidRenderInput {
            detail: format!(
                "the surface handle's single pointer is a {}, not this \
                 platform's appkit view pointer",
                set_platform_pointer.0.render_input_name()
            ),
        });
    }
    let appkit_view_pointer = this_platform_pointer(surface_handle.appkit_view_pointer)?;
    let raw_window_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(
        validated_platform_pointer(appkit_view_pointer),
    ));
    Ok(wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::AppKit(AppKitDisplayHandle::new())),
        raw_window_handle,
    })
}

#[cfg(target_os = "ios")]
fn this_platform_surface_target(
    surface_handle: &SurfaceHandle,
    set_platform_pointer: (PlatformPointerKind, u64),
) -> Result<wgpu::SurfaceTargetUnsafe, SdkError> {
    use raw_window_handle::{UiKitDisplayHandle, UiKitWindowHandle};
    if set_platform_pointer.0 != PlatformPointerKind::IosView {
        return Err(SdkError::InvalidRenderInput {
            detail: format!(
                "the surface handle's single pointer is a {}, not this \
                 platform's iOS view pointer",
                set_platform_pointer.0.render_input_name()
            ),
        });
    }
    let ios_view_pointer = this_platform_pointer(surface_handle.ios_view_pointer)?;
    let raw_window_handle = RawWindowHandle::UiKit(UiKitWindowHandle::new(
        validated_platform_pointer(ios_view_pointer),
    ));
    Ok(wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::UiKit(UiKitDisplayHandle::new())),
        raw_window_handle,
    })
}

#[cfg(target_os = "android")]
fn this_platform_surface_target(
    surface_handle: &SurfaceHandle,
    set_platform_pointer: (PlatformPointerKind, u64),
) -> Result<wgpu::SurfaceTargetUnsafe, SdkError> {
    use raw_window_handle::{AndroidDisplayHandle, AndroidNdkWindowHandle};
    if set_platform_pointer.0 != PlatformPointerKind::AndroidNativeWindow {
        return Err(SdkError::InvalidRenderInput {
            detail: format!(
                "the surface handle's single pointer is a {}, not this \
                 platform's Android native window pointer",
                set_platform_pointer.0.render_input_name()
            ),
        });
    }
    let android_native_window_pointer =
        this_platform_pointer(surface_handle.android_native_window_pointer)?;
    let raw_window_handle = RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(
        validated_platform_pointer(android_native_window_pointer),
    ));
    Ok(wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: Some(RawDisplayHandle::Android(AndroidDisplayHandle::new())),
        raw_window_handle,
    })
}

/// Every other target refuses at the boundary: the surface renderer exists
/// there, but no platform branch lowers a window handle,
/// so an attach attempt cannot pretend to have one.
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
fn this_platform_surface_target(
    _surface_handle: &SurfaceHandle,
    _set_platform_pointer: (PlatformPointerKind, u64),
) -> Result<wgpu::SurfaceTargetUnsafe, SdkError> {
    Err(SdkError::RenderingSurfaceUnavailable {
        detail: "this compilation target has no platform surface \
                 implementation; the map surface view supports macOS, iOS \
                 and Android"
            .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{PlatformPointerKind, surface_target_from_handle};
    use crate::records::SurfaceHandle;

    /// Whether a platform branch compiles for this target (macOS, iOS or
    /// Android); every other target refuses at the boundary with
    /// `RenderingSurfaceUnavailable` (see the fallback surface target).
    const THIS_TARGET_HAS_PLATFORM_BRANCH: bool = cfg!(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ));

    /// A fabricated non-zero pointer the validation rules check without the
    /// unsafe surface creation ever dereferencing it.
    const FABRICATED_POINTER_VALUE: u64 = 0x0000_0000_dead_beaf;

    /// Builds a handle carrying exactly one fabricated pointer, for
    /// `pointer_kind`.
    fn handle_with_pointer(pointer_kind: PlatformPointerKind) -> SurfaceHandle {
        match pointer_kind {
            PlatformPointerKind::AppKitView => SurfaceHandle {
                appkit_view_pointer: Some(FABRICATED_POINTER_VALUE),
                ios_view_pointer: None,
                android_native_window_pointer: None,
            },
            PlatformPointerKind::IosView => SurfaceHandle {
                appkit_view_pointer: None,
                ios_view_pointer: Some(FABRICATED_POINTER_VALUE),
                android_native_window_pointer: None,
            },
            PlatformPointerKind::AndroidNativeWindow => SurfaceHandle {
                appkit_view_pointer: None,
                ios_view_pointer: None,
                android_native_window_pointer: Some(FABRICATED_POINTER_VALUE),
            },
        }
    }

    /// This compiling target's own platform pointer, when a branch
    /// compiles for it (the fallback targets carry an arbitrary kind: their
    /// refusal fires before any platform check, so no pointer owns them).
    fn this_target_pointer_kind() -> PlatformPointerKind {
        if cfg!(target_os = "macos") {
            PlatformPointerKind::AppKitView
        } else if cfg!(target_os = "ios") {
            PlatformPointerKind::IosView
        } else if cfg!(target_os = "android") {
            PlatformPointerKind::AndroidNativeWindow
        } else {
            PlatformPointerKind::AppKitView
        }
    }

    /// A pointer kind foreign to the compiling target: an iOS pointer on
    /// macOS and Android builds, an appkit pointer on iOS builds (a
    /// fallback target refuses every kind before any platform check, so
    /// any kind is foreign there).
    fn foreign_pointer_kind() -> PlatformPointerKind {
        if cfg!(target_os = "ios") {
            PlatformPointerKind::AppKitView
        } else {
            PlatformPointerKind::IosView
        }
    }

    #[test]
    fn a_single_platform_pointer_lowers_onto_this_platform_target() {
        // The compiling build lowers its own platform pointer without
        // touching the object behind it: the wrap only assembles the raw
        // handle (an invalid pointer surfaces later, at configuration). A
        // build with no platform branch refuses its handle at the boundary
        // instead.
        let lowering = surface_target_from_handle(&handle_with_pointer(this_target_pointer_kind()));
        if THIS_TARGET_HAS_PLATFORM_BRANCH {
            assert!(
                lowering.is_ok(),
                "this platform's pointer must lower onto its surface target: {lowering:?}"
            );
        } else {
            assert!(matches!(
                lowering,
                Err(crate::error::SdkError::RenderingSurfaceUnavailable { .. })
            ));
        }
    }

    #[test]
    fn a_foreign_platform_pointer_refuses_instead_of_lowering() {
        // A pointer built for another platform must not lower onto this
        // target: a branch build names the wrong platform pointer, a
        // fallback build refuses the surface outright. Either way the
        // lowering refuses cleanly instead of guessing which pointer to
        // read.
        let failure = surface_target_from_handle(&handle_with_pointer(foreign_pointer_kind()))
            .expect_err("a foreign platform's pointer must not lower onto this target");
        if THIS_TARGET_HAS_PLATFORM_BRANCH {
            assert!(matches!(
                failure,
                crate::error::SdkError::InvalidRenderInput { .. }
            ));
        } else {
            assert!(matches!(
                failure,
                crate::error::SdkError::RenderingSurfaceUnavailable { .. }
            ));
        }
    }

    #[test]
    fn an_ambiguous_handle_refuses() {
        let ambiguous_handle = SurfaceHandle {
            appkit_view_pointer: Some(FABRICATED_POINTER_VALUE),
            ios_view_pointer: Some(FABRICATED_POINTER_VALUE),
            android_native_window_pointer: None,
        };
        let failure = surface_target_from_handle(&ambiguous_handle)
            .expect_err("a handle with two pointers must not lower");
        assert_is_invalid_render_input_with(failure, "exactly one platform pointer");
    }

    #[test]
    fn a_zero_pointer_refuses() {
        // The `u64 0` stand-in for a null pointer never reaches the unsafe
        // surface creation.
        let zero_handle = SurfaceHandle {
            appkit_view_pointer: Some(0),
            ios_view_pointer: None,
            android_native_window_pointer: None,
        };
        let failure =
            surface_target_from_handle(&zero_handle).expect_err("a zero pointer must not lower");
        assert_is_invalid_render_input_with(failure, "is zero");
    }

    /// Asserts the failure is the render-input variant carrying the verbatim
    /// description fragment.
    fn assert_is_invalid_render_input_with(failure: crate::error::SdkError, detail_fragment: &str) {
        match failure {
            crate::error::SdkError::InvalidRenderInput { detail } => {
                assert!(
                    detail.contains(detail_fragment),
                    "the detail must carry the refusal reason: {detail}"
                );
            }
            other_failure => panic!("the wrong variant lowered: {other_failure:?}"),
        }
    }

    /// Pins the pointer-kind naming the validation messages report.
    #[test]
    fn the_pointer_kinds_name_their_record_fields() {
        assert_eq!(
            PlatformPointerKind::AppKitView.render_input_name(),
            "appkit view pointer"
        );
        assert_eq!(
            PlatformPointerKind::IosView.render_input_name(),
            "iOS view pointer"
        );
        assert_eq!(
            PlatformPointerKind::AndroidNativeWindow.render_input_name(),
            "Android native window pointer"
        );
    }
}
