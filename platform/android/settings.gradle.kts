// Gradle root for the Android packaging slice: one module that produces the
// AAR consumed by Android applications (uniffi-generated Kotlin bindings plus
// one `librustic_maps.so` per supported ABI).
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "maps-sdk-android"

include(":maps-sdk-native")
include(":maps-sdk")
include(":smoke-jvm")
include(":reference-application")