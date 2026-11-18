/**
 * Metro configuration for the reference application shell.
 *
 * The shared demo artifact (`platform/reference-scenes.json`) and the shared
 * provider registry (`platform/providers.json`) are single sources for every
 * reference app (docs/ReferenceAppUX.md §8, docs/ProviderGenericity.md): the
 * RN shell consumes both through Metro `require`s, so the `platform/`
 * directory is a watch folder on top of the app root.
 *
 * Resolution: bare specifiers resolve through the app root's `node_modules`
 * first; the bindings workspace's own `node_modules` is the ordered second
 * home for its build-time packages (`@ubjs/*`). First-match wins, so the
 * app root keeps the one React Native copy; a plain hierarchical lookup
 * would instead pull a second, divergent React Native copy from the
 * `file:`-installed bindings package's transitive tree — the classic
 * monorepo fix: hierarchical lookup off, an ordered resolution-home list.
 * The other platforms' native trees stay blocked.
 */
const path = require('path');
const { getDefaultConfig, mergeConfig } = require('@react-native/metro-config');

const platformRoot = path.resolve(__dirname, '..', '..');
const escapedPlatformRoot = platformRoot.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const blockedPaths = [
  // The native reference apps: large native trees Metro must never enter.
  escapedPlatformRoot + '\\/(android|ios|web)\\/',
].map(pattern => new RegExp(pattern));

/**
 * Metro configuration
 * https://reactnative.dev/docs/metro
 *
 * @type {import('@react-native/metro-config').MetroConfig}
 */
const config = {
  watchFolders: [platformRoot],
  resolver: {
    blockList: blockedPaths,
    nodeModulesPaths: [
      path.resolve(__dirname, 'node_modules'),
      path.resolve(__dirname, '..', 'node_modules'),
    ],
    disableHierarchicalLookup: true,
  },
};

module.exports = mergeConfig(getDefaultConfig(__dirname), config);