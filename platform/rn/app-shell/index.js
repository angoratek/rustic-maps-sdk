/**
 * The reference application's entry point: the generated Rust bindings
 * install their JSI crate into Hermes at import time, so the app registers
 * with that import inside a guard (an emulator without the TurboModule must
 * reach the shell's honest failure states, not a red screen).
 */

import { AppRegistry } from 'react-native';
import { name as appName } from './app.json';

// The runtime bring-up probe: logged before anything else requires React
// Native so it observes the bare Hermes globals.
// eslint-disable-next-line @typescript-eslint/no-require-imports
require('./src/runtimeProbe');

// The shell imports the package's deep entries (`maps-sdk-rn/MapView` and
// `maps-sdk-rn/MapsSdk`) rather than the root entry: the root entry installs
// the generated JSI crate and runs its checksum gate at module scope, while
// the shell's App needs only the native-module seam those deep entries carry.
// The root entry's curated exports (`MapView`, `MapsSdk` and the generated
// contract namespace) are pinned by the package-root smoke test.

import App from './App';

AppRegistry.registerComponent(appName, () => App);