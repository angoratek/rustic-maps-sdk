/**
 * The reference application's root: the safe-area provider and the v2 screen
 * (docs/ReferenceAppUX.md).
 */

import React from 'react';
import { SafeAreaProvider } from 'react-native-safe-area-context';

import ReferenceAppScreen from './src/screens/ReferenceAppScreen';

// The plan search view stays reachable from the app root.
export { PlanSearchView } from './src/screens/PlanSearchView';

export default function App(): React.JSX.Element {
  return (
    <SafeAreaProvider>
      <ReferenceAppScreen />
    </SafeAreaProvider>
  );
}
