/**
 * The theme model of the React Native reference application
 * (docs/ReferenceAppUX.md §3): a three-way Light/Dark/Auto choice with Auto
 * the default, a light → dark → auto → light cycle whose label names the
 * next choice, live OS resolution while Auto, and a stored-value reader that
 * tolerates anything a previous version wrote.
 */

import {
  defaultThemeChoice,
  nextThemeChoice,
  resolvedThemeName,
  themeChoiceFromStoredValue,
  themeChoiceSwitchLabel,
} from '../src/theme';

test('Auto is the default choice of a device that never chose', () => {
  expect(defaultThemeChoice).toBe('auto');
  expect(themeChoiceFromStoredValue(null)).toBe('auto');
  expect(themeChoiceFromStoredValue(undefined)).toBe('auto');
  expect(themeChoiceFromStoredValue('sepia')).toBe('auto');
  expect(themeChoiceFromStoredValue('light')).toBe('light');
  expect(themeChoiceFromStoredValue('dark')).toBe('dark');
  expect(themeChoiceFromStoredValue('auto')).toBe('auto');
});

test('the toggle cycles light, dark and auto and names the next choice', () => {
  expect(nextThemeChoice('light')).toBe('dark');
  expect(nextThemeChoice('dark')).toBe('auto');
  expect(nextThemeChoice('auto')).toBe('light');
  expect(themeChoiceSwitchLabel('light')).toBe('Switch to dark theme');
  expect(themeChoiceSwitchLabel('dark')).toBe('Switch to auto theme');
  expect(themeChoiceSwitchLabel('auto')).toBe('Switch to light theme');
});

test('Auto follows the system scheme live while an explicit choice does not', () => {
  expect(resolvedThemeName('auto', 'dark')).toBe('dark');
  expect(resolvedThemeName('auto', 'light')).toBe('light');
  // A platform with no scheme signal resolves to the system default, light.
  expect(resolvedThemeName('auto', null)).toBe('light');
  expect(resolvedThemeName('light', 'dark')).toBe('light');
  expect(resolvedThemeName('dark', 'light')).toBe('dark');
});
