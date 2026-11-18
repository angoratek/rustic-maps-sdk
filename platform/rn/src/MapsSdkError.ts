/**
 * The one typed failure the package's JavaScript surface throws.
 *
 * Every native rejection on both halves carries a stable code: the package's
 * bridge modules reject `MapsSdkNativeFailure` for any SDK refusal and
 * `MapsSdkInvalidTimeToLive` for an invalid tile-cache time-to-live (Android's
 * `Promise.reject(code, detail)` and iOS's
 * `reject(code, detail, error)` both surface that code to JavaScript). This
 * module turns those rejections, and the package's own missing-native
 * refusal, into one error type so a consumer catches one shape and reads
 * `code` instead of matching message text.
 */

/** The stable codes every `MapsSdkError` carries. */
export const MapsSdkErrorCode = {
  /** The native half is absent: no native module is installed or linked. */
  NativeUnavailable: 'MapsSdkNativeUnavailable',
  /** A native SDK operation refused; `message` is the SDK's own detail. */
  NativeFailure: 'MapsSdkNativeFailure',
  /** A tile-cache time-to-live was not a valid value. */
  InvalidTimeToLive: 'MapsSdkInvalidTimeToLive',
} as const;

/** One value of {@link MapsSdkErrorCode}. */
export type MapsSdkErrorCodeValue =
  (typeof MapsSdkErrorCode)[keyof typeof MapsSdkErrorCode];

/**
 * One refused package operation.
 *
 * `code` is stable and machine-readable; `message` is the SDK's verbatim
 * detail (never re-worded); `nativeCause` is the original rejection value when
 * one exists, for diagnostics only.
 */
export class MapsSdkError extends Error {
  /** The stable failure code, one of {@link MapsSdkErrorCode}. */
  readonly code: string;

  /** The original native rejection value, when the failure came from one. */
  readonly nativeCause: unknown;

  /**
   * Builds one typed failure.
   *
   * @param code the stable failure code.
   * @param message the verbatim failure detail.
   * @param nativeCause the original native rejection value, when any.
   */
  constructor(code: string, message: string, nativeCause?: unknown) {
    super(message);
    this.name = 'MapsSdkError';
    this.code = code;
    this.nativeCause = nativeCause;
    // Keep `instanceof` working when the compiled output targets an older
    // Error builtin (React Native's Hermes and the Jest test environment).
    // Only a direct construction needs the reset: a subclass's prototype must
    // stay its own, or `instanceof` its class would fail.
    if (new.target === MapsSdkError) {
      Object.setPrototypeOf(this, MapsSdkError.prototype);
    }
  }

  /** The native half is absent: no operation that needs it can run. */
  static nativeUnavailable(): MapsSdkError {
    return new MapsSdkError(
      MapsSdkErrorCode.NativeUnavailable,
      'the native SDK is unavailable in this JavaScript environment',
    );
  }

  /**
   * Wraps one native rejection into this typed shape, preserving the bridge's
   * own `code` and `message` verbatim. An already-typed failure passes
   * through unchanged.
   *
   * @param error the rejection value.
   */
  static fromNative(error: unknown): MapsSdkError {
    if (error instanceof MapsSdkError) {
      return error;
    }
    const bridgeFailure =
      typeof error === 'object' && error !== null
        ? (error as { code?: unknown; message?: unknown })
        : {};
    const code =
      typeof bridgeFailure.code === 'string' && bridgeFailure.code.length > 0
        ? bridgeFailure.code
        : MapsSdkErrorCode.NativeFailure;
    const message =
      typeof bridgeFailure.message === 'string' &&
      bridgeFailure.message.length > 0
        ? bridgeFailure.message
        : String(error);
    return new MapsSdkError(code, message, error);
  }

  /** Whether the failure means the native half is not installed. */
  get isNativeUnavailable(): boolean {
    return this.code === MapsSdkErrorCode.NativeUnavailable;
  }
}

/**
 * Wraps a native module so every promise-returning method surfaces its bridge
 * rejection as {@link MapsSdkError}, preserving the native code and detail.
 * A non-promise member (a listener registration) passes through unchanged.
 *
 * @param module the native module object.
 * @returns the same surface with typed promise rejections.
 */
export function wrapNativeFailures<ModuleType extends object>(
  module: ModuleType,
): ModuleType {
  return new Proxy(module, {
    get(target, property, receiver) {
      const member: unknown = Reflect.get(target, property, receiver);
      if (typeof member !== 'function') {
        return member;
      }
      return (...argumentsList: unknown[]) => {
        try {
          const result: unknown = member.apply(target, argumentsList);
          return result instanceof Promise
            ? result.catch((nativeFailure: unknown) => {
                throw MapsSdkError.fromNative(nativeFailure);
              })
            : result;
        } catch (nativeFailure) {
          throw MapsSdkError.fromNative(nativeFailure);
        }
      };
    },
  }) as ModuleType;
}
