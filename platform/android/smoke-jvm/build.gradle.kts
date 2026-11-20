// Gradle root already supplies `google()` and `mavenCentral()`.
plugins {
    id("org.jetbrains.kotlin.jvm") version "2.2.21"
}

kotlin {
    compilerOptions {
        // Matches the Gradle JVM hosting the smoke run (Temurin 21).
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_21)
    }
}

sourceSets {
    main {
        // Reuse the uniffi-generated Kotlin bindings that the published AAR
        // carries so the smoke test exercises exactly the shipped surface.
        kotlin.srcDirs("../maps-sdk-native/src/main/kotlin")
    }
}

dependencies {
    implementation(kotlin("stdlib"))
    // The JVM (non-AAR) JNA artifact: the generated bindings drive every FFI
    // call through it.
    implementation("net.java.dev.jna:jna:5.18.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core-jvm:1.10.2")
}

tasks.register<JavaExec>("smokeRun") {
    mainClass = "com.rusticmaps.sdk.smoke.SmokeMainKt"
    classpath = sourceSets.main.get().runtimeClasspath
    // The host cdylib (`librustic_maps.dylib` on macOS, `.so` on Linux)
    // is copied here by scripts/build-android.sh; on the JVM host the
    // host-OS cdylib is loaded, since the Android `.so` files only run
    // inside Android.
    jvmArgs("-Djna.library.path=${projectDir}/jni", "-Djna.debug_load=false")
}