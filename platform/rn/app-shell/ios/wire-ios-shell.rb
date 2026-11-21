# The xcodeproj wiring for the RN reference application's iOS shell (run
# idempotently to re-wire the template app target):
#   ruby wire-ios-shell.rb /path/to/project.xcodeproj
# - compiles the shell's Swift sources into the app target; the uniffi Swift
#   bindings, the package MapView widget and the SDK runtime ship in the
#   MapsSdkRn pod and are imported from there
# - compiles the ObjC++ bridge glue (RefappMapModule)
# - adds the shared demo artifact and the trace fixtures as bundle resources
# - the pod's vendored archive supplies the Rust symbols at link time (the
#   same rustic-maps-native-core crate build the app target used to compile)
# - pins a distinct bundle identifier (the template default collides with
#   other RN example apps already installed on the standing simulator)
#
# The template app group (`MapsSdkRefappRn`) carries only a `name`, no
# `path`, so every file reference added to it must carry the target's own
# directory prefix (`MapsSdkRefappRn/<file>`) — the template's own
# AppDelegate reference does exactly that. External references sit in a
# pathless group too, so they carry the full relative path from the
# project's directory.
#
# SWIFT_INCLUDE_PATHS entries are bare quoted paths: the build system adds
# the `-I` flag itself for every entry (an embedded `-I` becomes a literal
# path component and the module search silently fails).
require "xcodeproj"

project_path = ARGV.fetch(0)
project = Xcodeproj::Project.open(project_path)

target = project.targets.fetch(0)
target_name = target.name

app_sources_group = project.main_group.find_subpath("MapsSdkRefappRn") ||
  project.main_group.new_group("MapsSdkRefappRn")
resources_group = app_sources_group.find_subpath("Resources") ||
  app_sources_group.new_group("Resources")
external_sources_group = project.main_group.find_subpath("ExternalSources") ||
  project.main_group.new_group("ExternalSources")

swift_source_paths = %w[
  RefappConstants.swift
  RefappScenesLoader.swift
  RefappTraceReplay.swift
  RefappSpeechSpeaker.swift
  RefappCameraEasing.swift
  RefappDemoSession.swift
]

# The uniffi Swift bindings now ship inside the MapsSdkRn pod (the package's
# iOS half compiles them from its `ios/generated/` symlink), so the app target
# imports that module instead of compiling a second copy.
external_source_paths = {}

objc_glue_paths = %w[RefappMapModule.mm]
objc_header_paths = %w[RefappMapModule.h]

resource_paths = {
  "reference-scenes.json" => "../../../reference-scenes.json",
}

# The trace fixtures bundle under their on-disk names (the fixture set is
# the navigation crate's; the sweep that re-anchors fixture recordings may
# rename entries — the wiring follows the directory, dropping stale names
# instead of failing the build on a deleted fixture).
trace_fixture_glob = "../../../../crates/rustic-maps-navigation/fixtures/navigation/traces/*.json"
Dir.glob(File.expand_path(trace_fixture_glob, Dir.pwd)).sort.each do |fixture_path|
  fixture_name = File.basename(fixture_path)
  resource_paths[fixture_name] = "../../../../crates/rustic-maps-navigation/fixtures/navigation/traces/#{fixture_name}"
end

def ensure_reference(group, file_name, path_value, in_build_phase = nil)
  file_reference = group.files.find { |f| f.name == file_name || f.path == file_name } ||
    group.files.find { |f| f.path == path_value } ||
    group.new_reference(path_value)
  # Repair pass (idempotent): a reference added earlier without the
  # directory prefix resolves against the project directory instead of the
  # file's real home.
  file_reference.name = file_name
  file_reference.path = path_value
  in_build_phase&.add_file_reference(file_reference, avoid_duplicates: true)
  file_reference
end

# Retired sources (the app-shell's map host/controller and its JNI-era host
# view, plus the app-target copy of the uniffi bindings the package pod now
# carries) are pruned idempotently, so a project wired by an older round does
# not keep compiling files that no longer exist.
retired_source_names = %w[
  RefappMapHostView.swift
  ReferenceMapController.swift
  RefappMapManager.mm
  RefappMapManager.h
  rustic_maps.swift
]
retired_source_names.each do |retired_name|
  project.files.select do |file_reference|
    file_reference.name == retired_name ||
      File.basename(file_reference.path.to_s) == retired_name
  end.each do |file_reference|
    target.source_build_phase.files.select do |build_file|
      build_file.file_ref == file_reference
    end.each(&:remove_from_project)
    file_reference.remove_from_project
  end
end

swift_source_paths.each do |swift_name|
  ensure_reference(
    app_sources_group,
    swift_name,
    "MapsSdkRefappRn/#{swift_name}",
    target.source_build_phase,
  )
end

objc_glue_paths.each do |glue_name|
  ensure_reference(
    app_sources_group,
    glue_name,
    "MapsSdkRefappRn/#{glue_name}",
    target.source_build_phase,
  )
end

objc_header_paths.each do |header_name|
  ensure_reference(app_sources_group, header_name, "MapsSdkRefappRn/#{header_name}")
end

external_source_paths.each do |source_name, source_path|
  ensure_reference(
    external_sources_group,
    source_name,
    source_path,
    target.source_build_phase,
  )
end

resource_paths.each do |resource_name, resource_path|
  ensure_reference(
    resources_group,
    resource_name,
    resource_path,
    target.resources_build_phase,
  )
end

# Drop resource references to trace fixtures the directory no longer carries
# (the shared artifact names the demo trace; a renamed fixture set must not
# wedge the build on a deleted file's reference).
kept_fixture_names = resource_paths.keys
resources_group.files.each do |resource_reference|
  next unless resource_reference.name&.end_with?(".json") &&
    resource_reference.name != "reference-scenes.json"
  unless kept_fixture_names.include?(resource_reference.name)
    removed = target.resources_build_phase.remove_file_reference(resource_reference)
    resource_reference.remove_from_project unless removed == false
    resource_reference.remove_from_project
  end
end

# The Swift module search for `import rustic_mapsFFI`, conditioned per
# SDK (both slices carry an identical modulemap; a single unconditioned
# search path would carry two same-named modulemaps at once). Bare quoted
# paths — the build system adds the `-I` flags. `$(inherited)` keeps the
# Pods xcconfig's own Swift search paths in the conditional variants: it
# carries `$(PODS_CONFIGURATION_BUILD_DIR)/MapsSdkRn`, without which
# `import MapsSdkRn` resolves only the pod's Clang module and none of the
# generated Swift types.
module_search_root = "$(SRCROOT)/../../../ios/artifacts/rustic_mapsFFI.xcframework"
target.build_configurations.each do |build_configuration|
  build_configuration.build_settings["SWIFT_INCLUDE_PATHS[sdk=iphonesimulator*]"] =
    "$(inherited) \"#{module_search_root}/ios-arm64-simulator/Headers\""
  build_configuration.build_settings["SWIFT_INCLUDE_PATHS[sdk=iphoneos*]"] =
    "$(inherited) \"#{module_search_root}/ios-arm64/Headers\""
  # The app's ObjC++ bridge imports the pod's generated Swift header,
  # `MapsSdkRn-Swift.h`, to reach the pod's Swift classes. Xcode 26 writes
  # that header below the pod product directory's `Swift Compatibility
  # Header` folder, so both the product directory and that folder join the
  # header search; `$(inherited)` keeps the Pods xcconfig's own paths.
  build_configuration.build_settings["HEADER_SEARCH_PATHS"] =
    "$(inherited) \"$(PODS_CONFIGURATION_BUILD_DIR)/MapsSdkRn\" " \
    "\"$(PODS_CONFIGURATION_BUILD_DIR)/MapsSdkRn/Swift Compatibility Header\""
  build_configuration.build_settings["PRODUCT_BUNDLE_IDENTIFIER"] =
    "com.rusticmaps.sdk.refapprn"
end

project.save

puts "wired #{target_name}: #{swift_source_paths.count} swift sources, " \
  "#{objc_glue_paths.count} objc++ glue files, #{resource_paths.count} resources, " \
  "per-sdk module search, bundle id com.rusticmaps.sdk.refapprn"
puts "source files now: #{target.source_build_phase.files.map(&:display_name).inspect}"