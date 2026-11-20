//! Decode of the committed synthetic Sourdough fixture tile
//! (`fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt`), checked
//! against the Sourdough schema's published layer names
//! (https://sourdough.osm.fyi/schema/ — the tileset's 32 `vector_layers` ids
//! are `advertising`, `aerialways`,
//! `aeroways`, `amenities`, `barriers`, `boundaries`, `buildings`,
//! `clubs`, `craft`, `education`, `emergency`, `geological`,
//! `healthcare`, `highways`, `historic`, `landcover`, `landuse`,
//! `leisure`, `man_made`, `military`, `natural`, `offices`, `pistes`,
//! `places`, `power`, `public_transport`, `railways`, `routes`, `shops`,
//! `tourism`, `water`, `waterways`).
//!
//! The fixture envelope pin (from the fixture README): the Tile message
//! leads with the repeated `layers` field (wire key `0x1a`) and the first
//! layer is `aeroways`. The bytes are a plain MVT protobuf payload the
//! repository generates with `node scripts/generate-openstreetmap-us-fixtures.mjs`;
//! the test never touches the network.

use rustic_maps_vector_tiles::{
    VectorTileAttributeValue, VectorTileGeometry, VectorTileGeometryType, decode_vector_tile,
};

/// The committed synthetic Sourdough MVT protobuf tile.
const SOURDOUGH_TILE_BYTES: &[u8] =
    include_bytes!("../../../fixtures/openstreetmap-us/sourdough_tile_z6_x18_y27.mvt");

/// Every layer name the Sourdough schema publishes for the tileset.
const SOURDOUGH_SCHEMA_LAYER_NAMES: [&str; 32] = [
    "advertising",
    "aerialways",
    "aeroways",
    "amenities",
    "barriers",
    "boundaries",
    "buildings",
    "clubs",
    "craft",
    "education",
    "emergency",
    "geological",
    "healthcare",
    "highways",
    "historic",
    "landcover",
    "landuse",
    "leisure",
    "man_made",
    "military",
    "natural",
    "offices",
    "pistes",
    "places",
    "power",
    "public_transport",
    "railways",
    "routes",
    "shops",
    "tourism",
    "water",
    "waterways",
];

#[test]
fn the_committed_sourdough_fixture_decodes_cleanly() {
    let tile =
        decode_vector_tile(SOURDOUGH_TILE_BYTES).expect("the committed Sourdough tile must decode");
    assert!(!tile.layers.is_empty());
    // The fixture README's envelope pin: the first layer is `aeroways`.
    assert_eq!(tile.layers[0].name, "aeroways");
}

#[test]
fn the_sourdough_tile_shape_matches_the_pinned_decode() {
    // Pinned against the synthetic generator's own scene: 11 layers in wire
    // order, one feature each, and all three compliant geometry kinds in
    // use. The layer set is exactly these Sourdough-schema layers.
    let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
    assert_eq!(tile.layers.len(), 11);
    let total_features: usize = tile.layers.iter().map(|layer| layer.features.len()).sum();
    assert_eq!(total_features, 11);
    let layer_names: Vec<&str> = tile
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect();
    assert_eq!(
        layer_names,
        vec![
            "aeroways",
            "boundaries",
            "highways",
            "landuse",
            "leisure",
            "natural",
            "places",
            "power",
            "railways",
            "routes",
            "water"
        ]
    );
    let mut geometry_types_seen = std::collections::HashSet::new();
    for layer in &tile.layers {
        for feature in &layer.features {
            geometry_types_seen.insert(feature.geometry_type);
        }
    }
    assert_eq!(geometry_types_seen.len(), 3);
    assert!(geometry_types_seen.contains(&VectorTileGeometryType::Point));
    assert!(geometry_types_seen.contains(&VectorTileGeometryType::LineString));
    assert!(geometry_types_seen.contains(&VectorTileGeometryType::Polygon));
}

#[test]
fn every_sourdough_layer_is_a_schema_layer() {
    let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
    for layer in &tile.layers {
        assert!(
            SOURDOUGH_SCHEMA_LAYER_NAMES.contains(&layer.name.as_str()),
            "layer \"{}\" is not in the Sourdough schema's published layer list",
            layer.name
        );
    }
}

#[test]
fn every_sourdough_layer_carries_the_specification_version_and_extent() {
    let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
    for layer in &tile.layers {
        assert_eq!(layer.version, 2, "layer {} must be version 2", layer.name);
        assert!(
            layer.extent > 0,
            "layer {} must declare an extent",
            layer.name
        );
    }
}

#[test]
fn the_sourdough_tile_carries_resolved_attributes() {
    let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
    let mut features_with_attributes = 0;
    for layer in &tile.layers {
        for feature in &layer.features {
            if !feature.attributes.is_empty() {
                features_with_attributes += 1;
                for (key, value) in &feature.attributes {
                    assert!(!key.is_empty());
                    if let VectorTileAttributeValue::Text(text) = value {
                        // Attribute text is valid UTF-8 by decode.
                        assert!(!text.is_empty());
                    }
                }
            }
        }
    }
    assert!(
        features_with_attributes > 0,
        "the Sourdough tile carries attribute-bearing features"
    );
}

#[test]
fn the_sourdough_tile_carries_decoded_geometries_of_every_kind() {
    let tile = decode_vector_tile(SOURDOUGH_TILE_BYTES).unwrap();
    let mut geometry_types_seen = std::collections::HashSet::new();
    let mut total_features = 0;
    for layer in &tile.layers {
        total_features += layer.features.len();
        for feature in &layer.features {
            geometry_types_seen.insert(feature.geometry_type);
            let vertex_count = match &feature.geometry {
                VectorTileGeometry::Points(points) => points.len(),
                VectorTileGeometry::LineStrings(line_strings) => {
                    line_strings.iter().map(|path| path.len()).sum()
                }
                VectorTileGeometry::Polygons(rings) => {
                    rings.iter().map(|ring| ring.vertices.len()).sum()
                }
            };
            // Every geometry of the declared kind decoded to at least one
            // vertex (features always carry geometry here).
            assert!(
                vertex_count > 0,
                "layer {} feature of kind {:?} decoded to no vertices",
                layer.name,
                feature.geometry_type
            );
        }
    }
    assert!(total_features > 0);
}
