/**
 * The shared provider registry (docs/ProviderGenericity.md): the RN shell
 * consumes `platform/providers.json` by name through a Metro `require`,
 * exactly the file the SwiftPM bundle, the Android Gradle copy task, the dev
 * server and the web playground carry, so the provider descriptors cannot
 * drift. The snake_case keys of the registry are consumed as-is; nothing is
 * rekeyed on the RN side.
 *
 * The registry carries descriptors only — never a credential value.
 */
import providerRegistryArtifact from '../../../providers.json';

/**
 * One provider descriptor of the registry's `providers` list: the settings
 * surface is driven by these rows, never a hard-coded provider field, so a
 * future provider is one registry entry plus one registration dispatch arm.
 */
export interface ProviderCredentialDescriptor {
  provider_key: string;
  display_name: string;
  /** The credential label; a keyless provider declares none. */
  credential_label?: string;
  /**
   * The capabilities this provider serves, under the registry's contract
   * names (`geocoding`, `routing`, `raster_tiles`, …). A settings row offers
   * a descriptor for a capability only when this list names it. Optional so a
   * hand-built descriptor without capabilities stays assignable; the shared
   * artifact always declares it.
   */
  served_contracts?: string[];
  /**
   * The raster-tile caching ceiling the provider's terms declare, in days.
   * A descriptor that declares one is the provider whose tiles the native
   * runtime wraps in the disk cache; it is also the only provider the
   * runtime's cache-installing credential path accepts.
   */
  raster_tile_cache_ceiling_days?: number;
  registration_available: {
    android: boolean;
    ios: boolean;
    react_native: boolean;
    wasm: boolean;
  };
  attribution_text: string;
  /** The provider's attribution/terms note (rendered under the field). */
  terms_note?: string;
  /**
   * A keyless routing provider's public demo deployment address, passed to
   * the contract as its deployment argument. Absent when the descriptor
   * publishes none; a keyless selection without it is refused rather than
   * pointed at a crate default.
   */
  demo_base_url?: string;
  /** The demo address's rate-limit/terms note, rendered under the address. */
  demo_base_url_note?: string;
  settings_hint: string;
  /**
   * The provider's raster-tile URL template (`{z}/{x}/{y}`), or absent when
   * it publishes no raster source of that shape. A descriptor with a
   * template serves its own base map without a credential through the
   * provider-neutral registration.
   */
  raster_url_template?: string;
  /** The provider's highest served tile zoom level. */
  maximum_zoom_level?: number;
  /**
   * The provider's vector-tile URL template (`{z}/{x}/{y}`), or absent when
   * it publishes no vector tileset.
   */
  url_template?: string;
  /** The tile schema the provider's vector tileset carries (`carto`, …). */
  vector_style?: string;
  /** The committed vector styles the registry offers for this provider. */
  vector_styles?: VectorStyleDescriptor[];
}

/**
 * One vector style a registry descriptor offers: the committed style id
 * the SDK paints the base with, the consumer-facing style name, the theme it
 * belongs to (`light`/`dark`), and the attribution line its design requires.
 */
export interface VectorStyleDescriptor {
  style_id: string;
  display_name: string;
  theme: string;
  attribution_text: string;
  /**
   * The provider-published style document this style paints with, or absent
   * when the SDK carries the style. A hosted style document is fetched with
   * the registered credential and kept for the session, so the application
   * ships no vendor style file.
   */
  document_url?: string;
}

/**
 * The styled vector base a keyless start opens on: the provider key and
 * committed style id the registry names
 * at its top level. An incomplete declaration resolves to nothing, and the
 * keyless start then keeps the credential-less raster source.
 */
export interface KeylessBaseMapDeclaration {
  provider_key: string;
  style_id: string;
}

export interface ProviderRegistry {
  schema_version: number;
  fixture_base_path: string;
  providers: ProviderCredentialDescriptor[];
  /** The keyless start's styled base, or absent when the artifact declares none. */
  keyless_base_map?: KeylessBaseMapDeclaration;
  /**
   * The reference apps' own `User-Agent` template (one `%s` slot for the
   * app's SDK version), or absent when the artifact declares none. It lives
   * in the shared artifact because all four surfaces identify themselves the
   * same way to the public tile services their base maps come from.
   */
  reference_app_user_agent_template?: string;
}

export const providerRegistry: ProviderRegistry =
  providerRegistryArtifact as ProviderRegistry;

/**
 * The registry's provider descriptors in presentation order (one settings row
 * each; the validation and registration-availability decisions live in
 * `providerCredentials.ts`).
 */
export const providerDescriptors: ProviderCredentialDescriptor[] =
  providerRegistry.providers ?? [];

/**
 * The attribution notice the base map actually serving the map contributes.
 * It follows the serving descriptor, never the union of every registrable
 * provider: a base map switch must repaint the attribution too. Empty when
 * the descriptor declares no line (the notice renders nothing rather than a
 * guess) or when no base map is offerable.
 */
export function providerAttributionText(
  descriptor: ProviderCredentialDescriptor | null,
): string {
  const attributionText = descriptor?.attribution_text;
  return typeof attributionText === 'string' && attributionText.trim() !== ''
    ? attributionText
    : '';
}

/**
 * The terms notes the attribution panel lists under the attribution line:
 * the serving base map's terms note, or empty when the descriptor declares
 * none (the panel then renders the attribution line alone rather than a
 * guess).
 */
export function providerAttributionTermsNotes(
  descriptor: ProviderCredentialDescriptor | null,
): string[] {
  const termsNote = descriptor?.terms_note;
  return typeof termsNote === 'string' && termsNote.trim() !== ''
    ? [termsNote]
    : [];
}

/** The one `%s` slot of the artifact's `User-Agent` template. */
const userAgentVersionPlaceholder = '%s';

/**
 * The reference apps' own `User-Agent`, formatted from the shared artifact's
 * template with the running SDK version. Public tile services identify their
 * clients by it (the public tile services' usage policies refuse clients
 * that send nothing). Empty when the artifact declares no template: the
 * contract then sends no header rather than a shell literal.
 */
export function providerReferenceAppUserAgent(sdkVersion: string): string {
  const template = providerRegistry.reference_app_user_agent_template;
  if (typeof template !== 'string' || template.trim() === '') {
    return '';
  }
  return template.replace(userAgentVersionPlaceholder, sdkVersion);
}
