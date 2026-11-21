/**
 * The pure decisions of the reference application's base-map switch (M9's
 * native base-map switch): which registry descriptors the settings sheet
 * offers, whether each can serve yet, which one is serving, and what one
 * row's act registers. The screen owns the effectful half (the SDK call and
 * the map view's reload); this module stays free of React and native imports
 * so jest can pin every decision without a native side.
 *
 * The rows come from the shared provider registry, so a second base map is a
 * descriptor entry and nothing here changes.
 */

import type {
  UrlTemplateRasterImageFormat,
  UrlTemplateRasterTileSourceDefinition,
  UrlTemplateTileResolution,
  UrlTemplateVectorTileSource,
} from '@angoratek/rustic-maps-sdk/MapsSdk';

import type {
  KeylessBaseMapDeclaration,
  ProviderCredentialDescriptor,
  VectorStyleDescriptor,
} from './providerRegistry';

/** The lowest zoom level every registry raster base map declares (zoom 0). */
export const baseMapMinimumZoomLevel = 0;

/**
 * The highest zoom level assumed when a descriptor omits one (the artifact's
 * own default): the source then serves zoom 0 only rather than an invented
 * ceiling.
 */
const baseMapDefaultMaximumZoomLevel = 0;

/** The contract's PNG image-format ordinal (generated `RasterImageFormat.Png`). */
const rasterImageFormatPng: UrlTemplateRasterImageFormat = 0;

/** The contract's standard-resolution ordinal (`RasterTileResolution.Standard`). */
const rasterTileResolutionStandard: UrlTemplateTileResolution = 0;

/** The registry's contract name for a provider's vector tileset. */
const vectorTilesRegistryContract = 'vector_tiles';

/**
 * One offered base map: the registry descriptor and whether it can serve
 * now. A credential provider is offered before its key is applied and stays
 * not servable (the row names what it needs) until then.
 */
export interface BaseMapChoice {
  descriptor: ProviderCredentialDescriptor;
  /** Whether the SDK can serve this source in the current session. */
  isServable: boolean;
}

/** What one base-map row's act registers. */
export type BaseMapRegistrationPlan =
  | { kind: 'urlTemplate'; source: UrlTemplateRasterTileSourceDefinition }
  | { kind: 'credentialedRestore'; providerKey: string };

/** Whether the SDK registers this provider on React Native today. */
function isRegistrationAvailable(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return descriptor.registration_available?.react_native === true;
}

/**
 * Whether the descriptor serves its own base map without a credential: a
 * registry descriptor that publishes a `{z}/{x}/{y}` raster URL template and
 * has a React Native registration arm. The provider-neutral URL-template
 * registration serves it, so no credential is involved.
 */
function offersCredentiallessBaseMap(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return (
    isRegistrationAvailable(descriptor) &&
    typeof descriptor.raster_url_template === 'string' &&
    descriptor.raster_url_template.trim() !== ''
  );
}

/** Whether the descriptor is a credential provider with a registration arm. */
function isCredentialProvider(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  return (
    isRegistrationAvailable(descriptor) &&
    typeof descriptor.credential_label === 'string' &&
    descriptor.credential_label.trim() !== ''
  );
}

/**
 * The base maps the settings surface offers, in registry order: every
 * descriptor that publishes a credential-less raster source (always
 * servable), and every credential provider (offered before its key is
 * applied, servable only once that key is in `appliedProviderKeys`). A
 * descriptor with no React Native registration arm is omitted.
 */
export function baseMapChoices(
  descriptors: readonly ProviderCredentialDescriptor[],
  appliedProviderKeys: ReadonlySet<string>,
): BaseMapChoice[] {
  const choices: BaseMapChoice[] = [];
  for (const descriptor of descriptors) {
    if (offersCredentiallessBaseMap(descriptor)) {
      choices.push({ descriptor, isServable: true });
    } else if (isCredentialProvider(descriptor)) {
      choices.push({
        descriptor,
        isServable: appliedProviderKeys.has(descriptor.provider_key),
      });
    }
  }
  return choices;
}

/**
 * The base map actually serving the map: the user's selection while it can
 * still serve, otherwise the first servable choice. Null when nothing can
 * serve — a keyless session whose registry declares no credential-less
 * source — and the band then claims no provider at all.
 */
export function activeBaseMap(
  choices: readonly BaseMapChoice[],
  selectedProviderKey: string | null,
): ProviderCredentialDescriptor | null {
  const servableChoices = choices.filter(choice => choice.isServable);
  if (selectedProviderKey !== null) {
    const selected = servableChoices.find(
      choice => choice.descriptor.provider_key === selectedProviderKey,
    );
    if (selected !== undefined) {
      return selected.descriptor;
    }
  }
  return servableChoices.length > 0 ? servableChoices[0].descriptor : null;
}

/**
 * The registration one base-map row performs: the URL-template record the
 * SDK's provider-neutral registration takes (a raster source with no vendor
 * code), or — for a credentialed descriptor — the marker that its credential
 * must be re-applied instead. The `User-Agent` is the reference application's
 * own, formatted from the shared artifact and passed in by the call site.
 */
export function baseMapRegistrationPlan(
  descriptor: ProviderCredentialDescriptor,
  userAgentText: string,
): BaseMapRegistrationPlan {
  const urlTemplate = descriptor.raster_url_template;
  if (typeof urlTemplate === 'string' && urlTemplate.trim() !== '') {
    return {
      kind: 'urlTemplate',
      source: {
        urlTemplate,
        sourceName: descriptor.provider_key,
        minimumZoomLevel: baseMapMinimumZoomLevel,
        maximumZoomLevel:
          descriptor.maximum_zoom_level ?? baseMapDefaultMaximumZoomLevel,
        imageFormat: rasterImageFormatPng,
        tileResolution: rasterTileResolutionStandard,
        userAgent: userAgentText,
      },
    };
  }
  return { kind: 'credentialedRestore', providerKey: descriptor.provider_key };
}

/**
 * One offered styled vector base: a registry descriptor's vector
 * tileset painted with one of the committed styles the same descriptor lists.
 */
export interface VectorBaseMapChoice {
  descriptor: ProviderCredentialDescriptor;
  style: VectorStyleDescriptor;
  /**
   * Whether the choice can install right now. False while a credentialed
   * family's key is not applied: the row is still listed, with its
   * `Needs <credential label>` state, exactly as the credential-less raster
   * rows are.
   */
  isServable: boolean;
}

/**
 * The styled vector bases the settings sheet offers, in registry order and
 * style order: every descriptor publishing a vector tileset URL template plus
 * a `vector_styles` list contributes one row per style, and every descriptor
 * whose `vector_styles` name provider-published style documents contributes
 * its rows too. A credentialed family whose key is not applied is listed with
 * `isServable: false` so the row states `Needs <credential label>` instead of
 * disappearing. The SDK serves the template rows through the shipped map
 * view's committed-style member and the hosted rows through its
 * provider-document member.
 */
export function vectorBaseMapChoices(
  descriptors: readonly ProviderCredentialDescriptor[],
  appliedProviderKeys: ReadonlySet<string> = new Set(),
): VectorBaseMapChoice[] {
  // Keyless families first (the demo's default base among them), then the
  // credential-gated families, each in registry order.
  const keylessDescriptors = descriptors.filter((descriptor) => {
    const vectorUrlTemplate = descriptor.url_template;
    return (
      typeof vectorUrlTemplate === 'string' &&
      vectorUrlTemplate.trim() !== ''
    );
  });
  const hostedDescriptors = descriptors.filter(
    (descriptor) =>
      !keylessDescriptors.includes(descriptor) &&
      offersHostedVectorFamily(descriptor),
  );
  const choices: VectorBaseMapChoice[] = [];
  for (const descriptor of [...keylessDescriptors, ...hostedDescriptors]) {
    const isServable =
      keylessDescriptors.includes(descriptor) ||
      appliedProviderKeys.has(descriptor.provider_key);
    for (const style of descriptor.vector_styles ?? []) {
      choices.push({ descriptor, style, isServable });
    }
  }
  return choices;
}

/**
 * Whether a descriptor's vector family paints from style documents the
 * provider publishes: it declares the vector-tile contract, this platform
 * registers it, and it names at least one style. The session's credential
 * decides whether the row can install, not whether it is listed.
 */
function offersHostedVectorFamily(
  descriptor: ProviderCredentialDescriptor,
): boolean {
  const servesVectorTiles = (descriptor.served_contracts ?? []).includes(
    vectorTilesRegistryContract,
  );
  return (
    servesVectorTiles &&
    descriptor.registration_available.react_native &&
    (descriptor.vector_styles ?? []).length > 0
  );
}

/** One choice's stable identity (`provider/style`), the value its row carries. */
export function vectorBaseChoiceKey(choice: VectorBaseMapChoice): string {
  return `${choice.descriptor.provider_key}/${choice.style.style_id}`;
}

/**
 * The registry's declared keyless styled base, resolved among the offered
 * vector choices. Null when the registry
 * declares none, or when the declaration names a provider/style this build
 * does not offer — the keyless start then keeps the credential-less raster
 * source rather than installing nothing.
 */
export function keylessVectorBaseChoice(
  descriptors: readonly ProviderCredentialDescriptor[],
  declared: KeylessBaseMapDeclaration | null | undefined,
): VectorBaseMapChoice | null {
  if (
    declared === null ||
    declared === undefined ||
    declared.provider_key.trim() === '' ||
    declared.style_id.trim() === ''
  ) {
    return null;
  }
  return (
    vectorBaseMapChoices(descriptors).find(
      choice =>
        choice.isServable &&
        choice.descriptor.provider_key === declared.provider_key &&
        choice.style.style_id === declared.style_id,
    ) ?? null
  );
}

/** The consumer-facing name the base-map row shows for one choice. */
export function vectorBaseChoiceLabel(choice: VectorBaseMapChoice): string {
  return `${choice.descriptor.display_name} · ${choice.style.display_name}`;
}

/**
 * The attribution line a styled vector base must carry: the style's own
 * design line, falling back to the provider's general line when the style
 * declares none.
 */
export function vectorBaseAttributionText(choice: VectorBaseMapChoice): string {
  const styleAttribution = choice.style.attribution_text;
  if (typeof styleAttribution === 'string' && styleAttribution.trim() !== '') {
    return styleAttribution;
  }
  return choice.descriptor.attribution_text;
}

/**
 * The record one row installs when the provider publishes the style document:
 * the provider key and style label the SDK reports, and the credential-free
 * document address. The provider's own client applies the credential, so the
 * application never handles it. Null when the SDK carries the style instead.
 */
export interface HostedVectorBaseInstall {
  source: { providerKey: string; styleId: string };
  styleDocumentUrl: string;
}

/** The provider-published install a choice plans, or null for a carried style. */
export function hostedVectorBaseInstall(
  choice: VectorBaseMapChoice,
): HostedVectorBaseInstall | null {
  const styleDocumentUrl = choice.style.document_url?.trim() ?? '';
  if (styleDocumentUrl === '') {
    return null;
  }
  return {
    source: {
      providerKey: choice.descriptor.provider_key,
      styleId: choice.style.style_id,
    },
    styleDocumentUrl,
  };
}

/**
 * The record one styled-vector-base row installs on the shipped map view
 * the descriptor's vector tileset, the chosen committed style id, the
 * style's attribution line and the descriptor's zoom ceiling. A hosted style
 * installs through `hostedVectorBaseInstall` instead.
 */
export function vectorBaseSource(
  choice: VectorBaseMapChoice,
): UrlTemplateVectorTileSource {
  return {
    urlTemplate: choice.descriptor.url_template ?? '',
    sourceName: choice.descriptor.provider_key,
    attributionText: vectorBaseAttributionText(choice),
    styleId: choice.style.style_id,
    minimumZoomLevel: baseMapMinimumZoomLevel,
    maximumZoomLevel:
      choice.descriptor.maximum_zoom_level ?? baseMapDefaultMaximumZoomLevel,
  };
}
