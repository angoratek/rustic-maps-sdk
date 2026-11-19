// The Compose reference application (PRD §4.9, FR-I; docs/ReferenceAppUX.md):
// map display on the `MapView` surface, geocode search, route
// planning, and simulated turn-by-turn navigation driven by the recorded
// synthetic walk trace, all styled by the design tokens of the `:maps-sdk`
// wrapper module. The application ships keyless: provider credentials are
// entered at runtime from the settings surface (docs/ReferenceAppUX.md §6.5),
// never embedded at build time.
plugins {
    id("com.android.application") version "9.0.0"
    id("org.jetbrains.kotlin.plugin.compose") version "2.2.21"
}

// The repository root two levels above `platform/android`: the single sources
// the asset copy tasks bundle without any committed duplicate.
val repositoryRootDirectory: String = rootDir.parentFile.parentFile.absolutePath

tasks.register<Copy>("copyReferenceScenesArtifact") {
    // The shared demo artifact (docs/ReferenceAppUX.md §9): one language-
    // neutral source of the demo camera presets, overlay scenes and the
    // navigation demo description, copied at build time so the bundled asset
    // cannot drift from the file the web refapp consumes.
    from(File(repositoryRootDirectory, "platform/reference-scenes.json"))
    rename { "reference-scenes.json" }
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyReferenceFlowsArtifact") {
    // The shared reference-application flow schema
    // (`docs/ReferenceAppUX.md`): one language-neutral
    // source of the ordered flows, their copy IDs and their per-platform
    // automation hooks, copied at build time so the bundled asset cannot drift
    // from the file `scripts/check-reference-flows.mjs` reads.
    from(File(repositoryRootDirectory, "platform/reference-flows.json"))
    rename { "reference-flows.json" }
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyProviderRegistry") {
    // The shared provider registry (platform/providers.json): one
    // language-neutral source of provider descriptors, credential labels,
    // availability, attribution and hints, copied at build time so the
    // bundled asset cannot drift from the file the dev server and the other
    // refapps consume. Descriptors carry no credential values.
    from(File(repositoryRootDirectory, "platform/providers.json"))
    rename { "providers.json" }
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyNavigationTraceCleanWalk") {
    // The recorded synthetic walk trace the simulated navigation drives. The
    // file stays the single committed source (the byte-equality guard of
    // `rustic-maps-navigation` locks it); the app consumes the identical data.
    from(File(repositoryRootDirectory, "crates/rustic-maps-navigation/fixtures/navigation/traces/sacramento_clean_walk.json"))
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyNavigationTraceDeviationRecovery") {
    // The deviation-and-recovery demo's recorded trace (round 9's demo-plan
    // picker): the same single committed source, bundled identically — the
    // deviation fixtures replay the off-route excursion and its recovery.
    from(File(repositoryRootDirectory, "crates/rustic-maps-navigation/fixtures/navigation/traces/sacramento_deviation_recovery.json"))
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyOfflineRoutingArchive") {
    // The committed compiled offline routing graph the offline card's route
    // control reads: the same bytes the wasm seam and the contract tests use,
    // bundled at build time so the app carries one committed test graph and
    // never records or fetches another.
    from(
        File(
            repositoryRootDirectory,
            "crates/rustic-maps-offline-graph-compiler/fixtures/rmog/routing_grid.rmog",
        ),
    )
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

tasks.register<Copy>("copyNavigationTraceDeviationReroute") {
    // The deviation-and-reroute demo's recorded trace (round 9): the trace
    // carries the replacement route the replay applies when the guidance
    // freezes — bundled identically from the committed source.
    from(File(repositoryRootDirectory, "crates/rustic-maps-navigation/fixtures/navigation/traces/sacramento_deviation_reroute.json"))
    into(layout.buildDirectory.dir("generated/reference-assets"))
}

android {
    namespace = "com.rusticmaps.sdk.referenceapplication"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.rusticmaps.sdk.referenceapplication"
        minSdk = 24
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        // The SDK cdylibs (including the shipped surface-pointer JNI shim)
        // come from the `:maps-sdk` AAR; the app builds no native code of its
        // own.
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        abortOnError = true
    }

    sourceSets {
        getByName("main") {
            assets.srcDir(layout.buildDirectory.dir("generated/reference-assets"))
            // The refapp-only UniFFI bindings and their per-ABI libraries are
            // generated by scripts/build-refapp-core.sh into this module, not
            // into the published `:maps-sdk` AAR.
            kotlin.srcDir("src/refapp-core/generated/kotlin")
            jniLibs.srcDir("src/refapp-core/generated/jniLibs")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

tasks.named("preBuild") {
    dependsOn(
        "copyReferenceScenesArtifact",
        "copyReferenceFlowsArtifact",
        "copyProviderRegistry",
        "copyNavigationTraceCleanWalk",
        "copyNavigationTraceDeviationRecovery",
        "copyNavigationTraceDeviationReroute",
        "copyOfflineRoutingArchive",
        "assertRefappCoreGeneratedPresent",
    )
}

// The reference application's own UniFFI bindings are produced by
// `scripts/build-refapp-core.sh` into `src/refapp-core/generated`; a direct
// Gradle invocation on a clean checkout would compile against nothing. Fail
// with the script that produces them instead of a wall of unresolved symbols.
val refappCoreGeneratedDirectories = listOf(
    layout.projectDirectory.dir("src/refapp-core/generated/kotlin"),
    layout.projectDirectory.dir("src/refapp-core/generated/jniLibs"),
)
val assertRefappCoreGeneratedPresent = tasks.register("assertRefappCoreGeneratedPresent") {
    doLast {
        val missingDirectories = refappCoreGeneratedDirectories
            .map { directory -> directory.asFile }
            .filter { directory -> !directory.isDirectory || directory.list().isNullOrEmpty() }
            .map { directory -> directory.relativeTo(rootDir).path }
        if (missingDirectories.isNotEmpty()) {
            throw GradleException(
                "the reference application's generated bindings are missing " +
                    "(${missingDirectories.joinToString()}); " +
                    "run scripts/build-refapp-core.sh android to generate them",
            )
        }
    }
}

dependencies {
    implementation(project(":maps-sdk"))
    // The refapp-only generated bindings call through JNA; the SDK's own
    // generated bindings bring the same runtime across the AAR boundary.
    implementation("net.java.dev.jna:jna:5.18.1@aar")
    // The JVM unit suite runs on the host, so it needs the non-AAR JNA
    // artifact with the host dispatch library the AAR does not carry.
    testImplementation("net.java.dev.jna:jna:5.18.1")

    testImplementation("junit:junit:4.13.2")
    // The real org.json implementation for pure-JVM tests of the remembered
    // record's codec: the unit-test runtime's stub android.jar answers JSON
    // calls as "not mocked".
    testImplementation("org.json:json:20180813")

    // The connected UI-automation suite (round 12): instrumentation drives the
    // same user-path methods the sheet's controls call (the Android analogue
    // of the iOS refapp's launch-argument demo automation) and asserts the
    // exact values the screen renders — deterministic state reads, no
    // screenshot waits, and no live provider calls from the tests.
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:core:1.6.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")

    val composeBom = platform("androidx.compose:compose-bom:2026.06.01")
    // The connected suite reads the Compose semantics tree (the removed
    // settings sections, the layers panel's scene home and the shared 40-unit
    // control diameter), so the androidTest source set needs the Compose test
    // rule and its assertions on the same BOM as the application.
    androidTestImplementation(composeBom)
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.13.0")
}

// The JVM unit suite drives the generated refapp-only bindings, whose native
// library is the host build `scripts/build-refapp-core.sh android` stages
// under this directory. JNA resolves `map_refapp_core` from `jna.library.path`
// on the test runtime; the Android instrumented tests load the per-ABI
// libraries from the APK's `jniLibs` instead.
tasks.withType<org.gradle.api.tasks.testing.Test>().configureEach {
    systemProperty(
        "jna.library.path",
        file("src/refapp-core/generated/host").absolutePath,
    )
}