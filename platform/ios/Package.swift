// swift-tools-version: 5.10
// The Maps SDK native bindings for iOS: the Rust static library packaged as
// an XCFramework (binary target `rustic_mapsFFI`) plus the uniffi-
// generated Swift sources. Regenerate both with `scripts/build-ios.sh`.
//
// The tools version is pinned to 5.10 so the package builds in Swift 5
// language mode: the generated bindings are written for that mode and do not
// yet guarantee Swift 6 strict-concurrency conformance.
//
// Packaging identity (docs/PackagingAndRelease.md §1 ruling 2): the package,
// its core product and its Swift module are `RusticMapsSDK`. The binary
// target keeps the generated `rustic_mapsFFI` name, because the
// uniffi-generated Swift sources import that module through `canImport`; the
// name follows the contract namespace.
import PackageDescription

/// Apple platforms the XCFramework slices cover; pinned to the oldest
/// release accepted by the Swift 5.10 tools version.
let supportedPlatforms: [SupportedPlatform] = [.iOS(.v17)]

/// Name of the Rust static library module (matches the modulemap inside the
/// XCFramework) and of the binary target importing it. Contract-bound: the
/// generated Swift bindings import it by this name.
let ffiModuleName = "rustic_mapsFFI"

/// The shipped Swift module/product identity.
let sdkModuleName = "RusticMapsSDK"

let package = Package(
    name: sdkModuleName,
    platforms: supportedPlatforms,
    products: [
        .library(name: sdkModuleName, targets: [sdkModuleName]),
        .library(name: "MapNativeCoreUI", targets: ["MapNativeCoreUI"]),
    ],
    targets: [
        .binaryTarget(
            name: ffiModuleName,
            path: "artifacts/rustic_mapsFFI.xcframework"
        ),
        .target(
            name: sdkModuleName,
            dependencies: [.target(name: ffiModuleName)],
            path: "Sources/RusticMapsSDK",
            // The disk-space required-reason declaration travels with the
            // module whose Rust core reads the free space before it evicts
            // cached tiles.
            resources: [.copy("PrivacyInfo.xcprivacy")]
        ),
        .target(
            name: "MapNativeCoreUI",
            dependencies: [.target(name: sdkModuleName)],
            path: "Sources/MapNativeCoreUI",
            // The shared provider registry, copied flat into the resource
            // bundle (a top-level copied folder named `Resources` would yield
            // the macOS bundle layout, which codesign refuses). Demo content
            // stays out of the shipped package: the reference applications
            // bundle `platform/reference-scenes.json` from its single source.
            resources: [
                .copy("Resources/providers.json"),
            ]
        ),
    ]
)