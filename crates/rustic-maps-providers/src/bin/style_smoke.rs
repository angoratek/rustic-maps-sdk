//! The opt-in live style smoke's compile and style step.
//!
//! `scripts/live-style-smoke.mjs` fetches a provider's published style
//! document, one vector tile and, when the document declares them, its sprite
//! pair. This binary is the SDK side of that maintainer tool: it compiles the
//! fetched document, decodes the fetched tile, styles the tile under the
//! document, installs the compiled style in a vector-base scheduler and prints
//! what the pipeline found. It never fetches anything and never sees the
//! credential, so the tool's network access stays in the script and no vendor
//! URL shape enters the SDK crates.
//!
//! Run it through the script, which passes the fetched files:
//!
//! ```text
//! node scripts/live-style-smoke.mjs --enable-network --provider mapbox --tile 14/2661/6286
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use geospatial_traits::{VectorTileProvider, VectorTileSourceDescriptor};
use geospatial_types::{VectorTileData, VectorTileEncoding};
use rustic_maps_rendering::labels::label_candidates_for_vector_tile;
use rustic_maps_rendering::sprite::SpriteAtlas;
use rustic_maps_rendering::vector_base::VectorBaseScheduler;
use rustic_maps_rendering::vector_style::{
    VectorTileAddress, overlay_primitives_for_vector_tile_at_zoom,
    vector_style_source_layer_mismatches, vector_tile_style_from_json,
};

/// The style id the installed scheduler reports; the tool only inspects it.
const SMOKE_STYLE_ID: &str = "live-style-smoke";

/// A scheduler provider that serves the one tile the script fetched.
struct FetchedTileProvider {
    tile_bytes: Vec<u8>,
    zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
}

#[async_trait::async_trait]
impl VectorTileProvider for FetchedTileProvider {
    fn source_descriptor(&self) -> VectorTileSourceDescriptor {
        VectorTileSourceDescriptor {
            source_name: "live-style-smoke".to_string(),
            attribution_text: String::new(),
            minimum_zoom_level: 0,
            maximum_zoom_level: 24,
            style_hints: Vec::new(),
        }
    }

    async fn fetch_vector_tile(
        &self,
        tile_x: u32,
        tile_y: u32,
        zoom_level: u8,
    ) -> Result<VectorTileData, geospatial_types::SDKError> {
        if (tile_x, tile_y, zoom_level) != (self.tile_x, self.tile_y, self.zoom_level) {
            return Err(geospatial_types::SDKError::ProviderRequestRejected(
                format!(
                    "the smoke provider serves only the fetched tile {zoom_level}/{tile_x}/{tile_y}"
                ),
            ));
        }
        Ok(VectorTileData {
            tile_x,
            tile_y,
            zoom_level,
            tile_encoding: VectorTileEncoding::MapboxVectorTile,
            tile_bytes: self.tile_bytes.clone(),
        })
    }
}

/// The arguments the script passes.
struct Arguments {
    style_file: PathBuf,
    tile_file: PathBuf,
    zoom_level: u8,
    tile_x: u32,
    tile_y: u32,
    sprite_index_file: Option<PathBuf>,
    sprite_sheet_file: Option<PathBuf>,
}

fn main() -> ExitCode {
    if std::env::args().any(|argument| argument == "--help" || argument == "-h") {
        print_usage();
        return ExitCode::SUCCESS;
    }
    let Some(arguments) = parse_arguments() else {
        return ExitCode::from(2);
    };
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("live style smoke: {detail}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    println!(
        "usage: style-smoke --style-file PATH --tile-file PATH \
         --zoom N --tile-x N --tile-y N [--sprite-index PATH --sprite-sheet PATH]"
    );
}

fn parse_arguments() -> Option<Arguments> {
    let mut style_file: Option<PathBuf> = None;
    let mut tile_file: Option<PathBuf> = None;
    let mut zoom_level: Option<u8> = None;
    let mut tile_x: Option<u32> = None;
    let mut tile_y: Option<u32> = None;
    let mut sprite_index_file: Option<PathBuf> = None;
    let mut sprite_sheet_file: Option<PathBuf> = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--style-file" => style_file = arguments.next().map(PathBuf::from),
            "--tile-file" => tile_file = arguments.next().map(PathBuf::from),
            "--sprite-index" => sprite_index_file = arguments.next().map(PathBuf::from),
            "--sprite-sheet" => sprite_sheet_file = arguments.next().map(PathBuf::from),
            "--zoom" => zoom_level = arguments.next().and_then(|value| value.parse().ok()),
            "--tile-x" => tile_x = arguments.next().and_then(|value| value.parse().ok()),
            "--tile-y" => tile_y = arguments.next().and_then(|value| value.parse().ok()),
            "--help" | "-h" => print_usage(),
            other => {
                eprintln!("live style smoke: unknown argument {other}");
                return None;
            }
        }
    }
    match (style_file, tile_file, zoom_level, tile_x, tile_y) {
        (Some(style_file), Some(tile_file), Some(zoom_level), Some(tile_x), Some(tile_y)) => {
            Some(Arguments {
                style_file,
                tile_file,
                zoom_level,
                tile_x,
                tile_y,
                sprite_index_file,
                sprite_sheet_file,
            })
        }
        _ => {
            eprintln!(
                "live style smoke: --style-file, --tile-file, --zoom, --tile-x and --tile-y \
                 are required"
            );
            None
        }
    }
}

fn run(arguments: &Arguments) -> Result<(), String> {
    let style_document =
        std::fs::read_to_string(&arguments.style_file).map_err(|read_failure| {
            format!(
                "{} is unreadable: {read_failure}",
                arguments.style_file.display()
            )
        })?;
    let tile_bytes = std::fs::read(&arguments.tile_file).map_err(|read_failure| {
        format!(
            "{} is unreadable: {read_failure}",
            arguments.tile_file.display()
        )
    })?;
    let style = vector_tile_style_from_json(&style_document)
        .map_err(|style_failure| format!("the fetched style does not compile: {style_failure}"))?;
    let tile = rustic_maps_vector_tiles::decode_vector_tile(&tile_bytes)
        .map_err(|decode_failure| format!("the fetched tile does not decode: {decode_failure}"))?;
    let tile_address = VectorTileAddress {
        zoom_level: arguments.zoom_level,
        tile_x: arguments.tile_x,
        tile_y: arguments.tile_y,
    };

    println!("style layers: {}", style.layer_count());
    println!("style diagnostics: {}", style.unsupported.len());
    for diagnostic in &style.unsupported {
        println!(
            "  {} {}: {}",
            diagnostic.scope.description(),
            diagnostic.property_name,
            diagnostic.detail
        );
    }
    let feature_count: usize = tile
        .layers
        .iter()
        .map(|tile_layer| tile_layer.features.len())
        .sum();
    println!("tile layers: {}", tile.layers.len());
    println!("tile features: {feature_count}");

    let mismatches = vector_style_source_layer_mismatches(&tile, &style);
    println!("source-layer mismatches: {}", mismatches.len());
    for mismatch in &mismatches {
        println!("  {}", mismatch.detail);
    }

    let batches = overlay_primitives_for_vector_tile_at_zoom(
        &tile,
        tile_address,
        &style,
        f64::from(arguments.zoom_level),
    );
    let overlay_count: usize = batches.iter().map(|batch| batch.overlays.len()).sum();
    println!("painted batches: {}", batches.len());
    println!("painted overlays: {overlay_count}");
    for batch in &batches {
        println!(
            "  layer {:?}: {} overlays",
            batch.layer_id,
            batch.overlays.len()
        );
    }

    let labels = label_candidates_for_vector_tile(
        &tile,
        tile_address,
        &style,
        f64::from(arguments.zoom_level),
    );
    let icon_count = labels
        .iter()
        .filter(|candidate| candidate.icon.is_some())
        .count();
    println!("label candidates: {}", labels.len());
    println!("icon candidates: {icon_count}");

    match (&arguments.sprite_index_file, &arguments.sprite_sheet_file) {
        (Some(sprite_index_file), Some(sprite_sheet_file)) => {
            let sprite_index = std::fs::read(sprite_index_file).map_err(|read_failure| {
                format!(
                    "{} is unreadable: {read_failure}",
                    sprite_index_file.display()
                )
            })?;
            let sprite_sheet = std::fs::read(sprite_sheet_file).map_err(|read_failure| {
                format!(
                    "{} is unreadable: {read_failure}",
                    sprite_sheet_file.display()
                )
            })?;
            let sprite_atlas = SpriteAtlas::from_index_and_sheet_png(&sprite_index, &sprite_sheet)
                .map_err(|decode_failure| {
                    format!("the fetched sprite does not decode: {decode_failure}")
                })?;
            let resolvable_icons = labels
                .iter()
                .filter_map(|candidate| candidate.icon.as_ref())
                .filter(|icon| sprite_atlas.icon(&icon.image_name).is_some())
                .count();
            println!(
                "sprite sheet: {}x{} pixels",
                sprite_atlas.width_pixels(),
                sprite_atlas.height_pixels()
            );
            println!("resolvable icon candidates: {resolvable_icons}");
        }
        (None, None) => println!("sprite icons: not fetched"),
        _ => return Err("--sprite-index and --sprite-sheet must be given together".to_string()),
    }

    // Installing the compiled style in the vector-base scheduler proves the
    // document is usable by the base-map path, GPU-free.
    let provider = Arc::new(FetchedTileProvider {
        tile_bytes,
        zoom_level: arguments.zoom_level,
        tile_x: arguments.tile_x,
        tile_y: arguments.tile_y,
    });
    let scheduler = VectorBaseScheduler::new(provider, SMOKE_STYLE_ID, style);
    println!("installed style id: {}", scheduler.style_id());
    println!(
        "installed style layers: {}",
        scheduler.style().layer_count()
    );
    Ok(())
}
