#import <React/RCTViewManager.h>

// The pod's generated Swift header (the same pod compiles the Swift host view
// and the package runtime).
#import "RusticMapsSdk-Swift.h"

/**
 * The view manager exposing `MapsSdkMapHostView` to JavaScript as
 * `MapsSdkMapView` (the Android `MapsSdkMapViewManager` mirror).
 *
 * The Swift host owns the widget and the bridge glue; this manager only
 * creates the view and applies the declared props.
 */
@interface MapsSdkMapViewManager : RCTViewManager
@end

@implementation MapsSdkMapViewManager

RCT_EXPORT_MODULE(MapsSdkMapView)

- (UIView *)view {
  return [[MapsSdkMapHostView alloc] initWithFrame:CGRectZero];
}

/**
 * Releases a managed host view's display link, surface and native objects
 * when React Native removes it from the hierarchy (the Android
 * `onDropViewInstance` mirror).
 */
- (void)dismantleUIView:(UIView *)uiView {
  if ([uiView isKindOfClass:[MapsSdkMapHostView class]]) {
    [(MapsSdkMapHostView *)uiView teardown];
  }
}

+ (BOOL)requiresMainQueueSetup {
  return YES;
}

RCT_CUSTOM_VIEW_PROPERTY(viewIdentifier, NSString, MapsSdkMapHostView) {
  view.viewIdentifier = json;
}

RCT_CUSTOM_VIEW_PROPERTY(topBarInsetPixels, CGFloat, MapsSdkMapHostView) {
  [view setTopBarInsetPixels:[json doubleValue]];
}

RCT_CUSTOM_VIEW_PROPERTY(bottomBarInsetPixels, CGFloat, MapsSdkMapHostView) {
  [view setBottomBarInsetPixels:[json doubleValue]];
}

RCT_CUSTOM_VIEW_PROPERTY(leftBarInsetPixels, CGFloat, MapsSdkMapHostView) {
  [view setLeftBarInsetPixels:[json doubleValue]];
}

RCT_CUSTOM_VIEW_PROPERTY(rightBarInsetPixels, CGFloat, MapsSdkMapHostView) {
  [view setRightBarInsetPixels:[json doubleValue]];
}

RCT_CUSTOM_VIEW_PROPERTY(isGestureEnabled, BOOL, MapsSdkMapHostView) {
  view.isGestureEnabled = [json boolValue];
}

RCT_CUSTOM_VIEW_PROPERTY(isRenderingPaused, BOOL, MapsSdkMapHostView) {
  [view setRenderingPaused:[json boolValue]];
}

@end
