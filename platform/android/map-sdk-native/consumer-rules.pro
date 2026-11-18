# R8 rules every consumer inherits through the published AAR. The SDK's
# Kotlin surface is ordinary JVM code, but two reflective seams must keep
# their exact names.

# The JNI surface-pointer shim compiled into librustic_maps.so exports
# fixed symbols named after this Kotlin object (see
# crates/rustic-maps-native-core/src/android_jni.rs). R8 must keep the class and its
# native declarations exactly as named, or the JVM cannot resolve them.
-keep class com.rusticmaps.sdk.NativeWindowBridge {
    native <methods>;
}

# JNA resolves its native entry points, callback proxies and structure
# layouts by reflection over its own classes; a minified consumer without
# these keeps fails at the first SDK call with a missing symbol. The
# `java.awt` warning is JNA's optional desktop path, never reached on
# Android.
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-dontwarn java.awt.**

# The generated UniFFI bindings register the Rust library's extern-C
# functions through JNA direct mapping: the `UniffiLib` and
# `IntegrityCheckingUniffiLib` holders, the callback vtables and the
# integrity checks are resolved by the JVM at run time and must survive
# renaming and stripping.
-keep class uniffi.** { *; }
