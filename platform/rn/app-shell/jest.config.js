/**
 * The reference shell's jest configuration.
 *
 * The package under test (`@angoratek/rustic-maps-sdk`) is a `file:..`
 * dependency that resolves through the shell's `node_modules` symlink to
 * `platform/rn`, which carries its own development `react` install. Mapping
 * React (and its JSX runtime) to the shell's single copy keeps a component
 * rendered from the package on one hook dispatcher; the preset's
 * `react-native` mappings are preserved verbatim.
 */
const preset = require('@react-native/jest-preset');

module.exports = {
  ...preset,
  // The screen mounts the SDK asynchronously; a cold CI runner can take
  // several seconds to flush that mount before the first assertion.
  testTimeout: 30_000,
  moduleNameMapper: {
    ...preset.moduleNameMapper,
    // The package under test is a `file:..` dependency: resolve it (and its
    // deep entry points) straight to the package sources so the suite does
    // not depend on how npm materialises the link in a given environment.
    '^@angoratek/rustic-maps-sdk$': '<rootDir>/../src/index.ts',
    '^@angoratek/rustic-maps-sdk/src/(.*)$': '<rootDir>/../src/$1',
    '^@angoratek/rustic-maps-sdk/(.*)$': '<rootDir>/../src/$1',
    '^react$': '<rootDir>/node_modules/react',
    '^react/jsx-runtime$': '<rootDir>/node_modules/react/jsx-runtime',
  },
};
