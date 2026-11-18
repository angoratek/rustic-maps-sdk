//! Geometry kind, shape and command-stream decoding (specification §4.3/4.4).
//!
//! The command stream decoder implements the Mapbox Vector Tile v2.1
//! geometry rules exactly: a command integer packs a command count into
//! the high 29 bits and a command id into the low 3 bits
//! (`CommandInteger = (id & 0x7) | (count << 3)`), each command consumes
//! `count * parameters` zigzag-transformed integer deltas, and every
//! position is relative to the running *cursor* rather than absolute.
//! Polygon rings are explicitly closed with `ClosePath`, whose implied
//! segment back to the ring's first vertex is documented on
//! [`VectorTileLinearRing`] rather than duplicated into the vertex list.

use crate::error::VectorTileError;

/// Command id 0b001: begin a new position (and, for `POINT` geometries,
/// each additional multipoint vertex).
pub(crate) const COMMAND_ID_MOVE_TO: u64 = 1;
/// Command id 0b010: continue the current linear path from the cursor.
pub(crate) const COMMAND_ID_LINE_TO: u64 = 2;
/// Command id 0b111: close the current polygon ring back to its first
/// vertex without moving the cursor.
pub(crate) const COMMAND_ID_CLOSE_PATH: u64 = 7;

/// The number of low bits a command id occupies in a command integer.
pub(crate) const COMMAND_ID_BIT_COUNT: u32 = 3;
/// The low-bit mask that extracts a command id from a command integer.
pub(crate) const COMMAND_ID_MASK: u64 = 0b0111;
/// The maximum command count the 29 high bits can carry.
pub(crate) const MAXIMUM_COMMAND_COUNT: u64 = (1 << 29) - 1;

/// The `GeomType` enum value for `UNKNOWN`.
pub(crate) const GEOMETRY_TYPE_UNKNOWN: u64 = 0;
/// The `GeomType` enum value for `POINT`.
pub(crate) const GEOMETRY_TYPE_POINT: u64 = 1;
/// The `GeomType` enum value for `LINESTRING`.
pub(crate) const GEOMETRY_TYPE_LINESTRING: u64 = 2;
/// The `GeomType` enum value for `POLYGON`.
pub(crate) const GEOMETRY_TYPE_POLYGON: u64 = 3;

/// A feature's declared `GeomType`, as the specification's enum names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VectorTileGeometryType {
    /// `GeomType::UNKNOWN` (0) — never emitted by compliant producers and
    /// refused by the decoder.
    Unknown,
    /// `GeomType::POINT` (1).
    Point,
    /// `GeomType::LINESTRING` (2).
    LineString,
    /// `GeomType::POLYGON` (3).
    Polygon,
}

impl VectorTileGeometryType {
    /// The `&'static str` name the spec spells the kind with, for error
    /// context strings.
    pub(crate) fn spec_name(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Point => "POINT",
            Self::LineString => "LINESTRING",
            Self::Polygon => "POLYGON",
        }
    }
}

/// One decoded geometry vertex, in tile-local extent coordinates.
///
/// Extent coordinates are integers inside the layer's `extent` grid; the
/// renderer projects them into the map (the web-mercator tile bounds
/// at `[extent, extent]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VectorTileGeometryPoint {
    /// X coordinate within the layer extent.
    pub x_coordinate: i64,
    /// Y coordinate within the layer extent.
    pub y_coordinate: i64,
}

/// One decoded polygon ring: the ring's boundary vertices in stream order,
/// without the closing vertex repeated.
///
/// The ring's final edge — from the last vertex back to
/// `vertices[0]` — is implied by the stream's `ClosePath` command; the
/// decoder records the vertices exactly as the specification's cursor
/// leaves them and describes the closing segment rather than duplicating
/// the first vertex. Winding order carries meaning: within the tile's
/// `(x right, y down)` coordinate system an exterior ring has positive
/// shoelace area and an interior ring (hole) has negative area.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VectorTileLinearRing {
    /// The ring's boundary vertices in stream order.
    pub vertices: Vec<VectorTileGeometryPoint>,
}

impl VectorTileLinearRing {
    /// The ring's closed vertex sequence: the boundary vertices with the
    /// first vertex appended, ready for a renderer that wants the closing
    /// edge spelled out.
    pub fn closed_vertices(&self) -> Vec<VectorTileGeometryPoint> {
        let mut closed = self.vertices.clone();
        if let Some(first_vertex) = self.vertices.first() {
            closed.push(*first_vertex);
        }
        closed
    }
}

/// A decoded feature geometry, in the decoded shape the specification's
/// command rules produce.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VectorTileGeometry {
    /// One or more points (`POINT` features).
    Points(Vec<VectorTileGeometryPoint>),
    /// One or more linestrings (`LINESTRING` features), each an open path
    /// of vertices in stream order.
    LineStrings(Vec<Vec<VectorTileGeometryPoint>>),
    /// One or more polygon rings (`POLYGON` features), in stream order.
    /// The first ring is the exterior; the assignment of interior rings
    /// (holes) to their exteriors follows the winding rule documented on
    /// [`VectorTileLinearRing`] (hole rings have negative area).
    Polygons(Vec<VectorTileLinearRing>),
}

/// Decodes the packed `geometry` command integer stream of a feature of
/// `geometry_type` into its decoded shape.
///
/// The command integers are the already-read varint values of the packed
/// `geometry` field. Every malformed stream — an unknown command id, a
/// zero command count, a truncated parameter sequence, a disallowed
/// command for the geometry kind, a cursor overflow, an unclosed polygon
/// ring — yields a typed [`VectorTileError`].
pub fn decode_geometry_command_stream(
    geometry_type: VectorTileGeometryType,
    command_integers: &[u64],
) -> Result<VectorTileGeometry, VectorTileError> {
    if geometry_type == VectorTileGeometryType::Unknown {
        return Err(VectorTileError::UnknownGeometryType { found: 0 });
    }
    match geometry_type {
        VectorTileGeometryType::Point => {
            let points = decode_point_stream(command_integers)?;
            Ok(VectorTileGeometry::Points(points))
        }
        VectorTileGeometryType::LineString => {
            let line_strings = decode_line_string_stream(command_integers)?;
            Ok(VectorTileGeometry::LineStrings(line_strings))
        }
        VectorTileGeometryType::Polygon => {
            let rings = decode_polygon_stream(command_integers)?;
            Ok(VectorTileGeometry::Polygons(rings))
        }
        VectorTileGeometryType::Unknown => Err(VectorTileError::UnknownGeometryType { found: 0 }),
    }
}

/// The running cursor position the specification's commands are relative
/// to: every decoded position is
/// `(cursor_x + ZigZag(dX), cursor_y + ZigZag(dY))`, and each defining
/// command moves the cursor to that position.
#[derive(Clone, Copy)]
struct GeometryCursor {
    cursor_x: i64,
    cursor_y: i64,
}

impl GeometryCursor {
    fn new() -> Self {
        Self {
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    /// Reads the two zigzag-decoded parameter deltas starting at
    /// `*command_index` and moves the cursor by them.
    fn step(
        &mut self,
        command_integers: &[u64],
        command_index: &mut usize,
        context: &'static str,
    ) -> Result<VectorTileGeometryPoint, VectorTileError> {
        let delta_x = *command_integers
            .get(*command_index)
            .ok_or(VectorTileError::GeometryParameterStreamTruncated { context })?;
        let delta_y = *command_integers
            .get(*command_index + 1)
            .ok_or(VectorTileError::GeometryParameterStreamTruncated { context })?;
        *command_index += 2;
        let cursor_x = self
            .cursor_x
            .checked_add(crate::wire::zigzag_decode(delta_x))
            .ok_or(VectorTileError::GeometryCoordinateOverflow)?;
        let cursor_y = self
            .cursor_y
            .checked_add(crate::wire::zigzag_decode(delta_y))
            .ok_or(VectorTileError::GeometryCoordinateOverflow)?;
        self.cursor_x = cursor_x;
        self.cursor_y = cursor_y;
        Ok(VectorTileGeometryPoint {
            x_coordinate: cursor_x,
            y_coordinate: cursor_y,
        })
    }

    /// The `LineTo` variant of [`step`](Self::step), additionally refusing
    /// the `(0, 0)` delta the specification forbids.
    fn step_line_to(
        &mut self,
        command_integers: &[u64],
        command_index: &mut usize,
    ) -> Result<VectorTileGeometryPoint, VectorTileError> {
        let delta_x_index = *command_index;
        let delta_x = *command_integers.get(delta_x_index).ok_or(
            VectorTileError::GeometryParameterStreamTruncated {
                context: "LineTo parameters",
            },
        )?;
        if delta_x == 0 && command_integers.get(delta_x_index + 1).copied() == Some(0) {
            return Err(VectorTileError::ZeroDeltaLineTo);
        }
        self.step(command_integers, command_index, "LineTo parameters")
    }
}

/// Decodes a `POINT` command stream: only `MoveTo` commands appear, each
/// whose occurrences each produce one multipoint vertex.
fn decode_point_stream(
    command_integers: &[u64],
) -> Result<Vec<VectorTileGeometryPoint>, VectorTileError> {
    let mut points = Vec::new();
    let mut cursor = GeometryCursor::new();
    let mut command_index = 0;
    while command_index < command_integers.len() {
        let command_integer = command_integers[command_index];
        command_index += 1;
        let (count, command_id) = unpack_command_integer(command_integer)?;
        match command_id {
            COMMAND_ID_MOVE_TO => {
                for _ in 0..count {
                    points.push(cursor.step(
                        command_integers,
                        &mut command_index,
                        "MoveTo parameters",
                    )?);
                }
            }
            COMMAND_ID_LINE_TO | COMMAND_ID_CLOSE_PATH => {
                return Err(VectorTileError::GeometryCommandNotAllowed {
                    command_id,
                    geometry_kind: VectorTileGeometryType::Point.spec_name(),
                });
            }
            _ => {
                return Err(VectorTileError::UnknownGeometryCommand { command_id });
            }
        }
    }
    Ok(points)
}

/// Decodes a `LINESTRING` command stream: `MoveTo` (count 1) starts each
/// linestring, `LineTo` extends it; the linestring ends at the next
/// `MoveTo` or at the end of the stream.
fn decode_line_string_stream(
    command_integers: &[u64],
) -> Result<Vec<Vec<VectorTileGeometryPoint>>, VectorTileError> {
    let mut line_strings = Vec::new();
    let mut current_path: Option<Vec<VectorTileGeometryPoint>> = None;
    let mut cursor = GeometryCursor::new();
    let mut command_index = 0;
    while command_index < command_integers.len() {
        let command_integer = command_integers[command_index];
        command_index += 1;
        let (count, command_id) = unpack_command_integer(command_integer)?;
        match command_id {
            COMMAND_ID_MOVE_TO => {
                if count > 1 {
                    return Err(VectorTileError::MultiVertexMoveToOutsidePoints {
                        count,
                        geometry_kind: VectorTileGeometryType::LineString.spec_name(),
                    });
                }
                let start =
                    cursor.step(command_integers, &mut command_index, "MoveTo parameters")?;
                if let Some(finished_path) = current_path.take() {
                    line_strings.push(finished_path);
                }
                current_path = Some(vec![start]);
            }
            COMMAND_ID_LINE_TO => {
                let path = current_path
                    .as_mut()
                    .ok_or(VectorTileError::LineToBeforeMoveTo)?;
                for _ in 0..count {
                    path.push(cursor.step_line_to(command_integers, &mut command_index)?);
                }
            }
            COMMAND_ID_CLOSE_PATH => {
                return Err(VectorTileError::GeometryCommandNotAllowed {
                    command_id,
                    geometry_kind: VectorTileGeometryType::LineString.spec_name(),
                });
            }
            _ => {
                return Err(VectorTileError::UnknownGeometryCommand { command_id });
            }
        }
    }
    if let Some(finished_path) = current_path.take() {
        line_strings.push(finished_path);
    }
    Ok(line_strings)
}

/// Decodes a `POLYGON` command stream: each ring is a `MoveTo` (count 1)
/// followed by `LineTo` commands and terminated by exactly one
/// `ClosePath`. Rings stay separate; interior rings keep negative-area
/// winding, which the renderer resolves per the specification's area rule.
fn decode_polygon_stream(
    command_integers: &[u64],
) -> Result<Vec<VectorTileLinearRing>, VectorTileError> {
    let mut rings = Vec::new();
    let mut current_ring: Option<Vec<VectorTileGeometryPoint>> = None;
    let mut cursor = GeometryCursor::new();
    let mut command_index = 0;
    while command_index < command_integers.len() {
        let command_integer = command_integers[command_index];
        command_index += 1;
        let (count, command_id) = unpack_command_integer(command_integer)?;
        match command_id {
            COMMAND_ID_MOVE_TO => {
                if count > 1 {
                    return Err(VectorTileError::MultiVertexMoveToOutsidePoints {
                        count,
                        geometry_kind: VectorTileGeometryType::Polygon.spec_name(),
                    });
                }
                if current_ring.is_some() {
                    return Err(VectorTileError::PolygonRingUnclosed);
                }
                let start =
                    cursor.step(command_integers, &mut command_index, "MoveTo parameters")?;
                current_ring = Some(vec![start]);
            }
            COMMAND_ID_LINE_TO => {
                let ring = current_ring
                    .as_mut()
                    .ok_or(VectorTileError::LineToBeforeMoveTo)?;
                for _ in 0..count {
                    ring.push(cursor.step_line_to(command_integers, &mut command_index)?);
                }
            }
            COMMAND_ID_CLOSE_PATH => {
                if count != 1 {
                    return Err(VectorTileError::ClosePathCommandCountNotOne { found: count });
                }
                let ring = current_ring
                    .take()
                    .ok_or(VectorTileError::LineToBeforeMoveTo)?;
                rings.push(VectorTileLinearRing { vertices: ring });
            }
            _ => {
                return Err(VectorTileError::UnknownGeometryCommand { command_id });
            }
        }
    }
    if current_ring.is_some() {
        return Err(VectorTileError::PolygonRingUnclosed);
    }
    Ok(rings)
}

/// Splits a command integer into `(count, command_id)` per the
/// specification's packing, refusing a count of 0, counts beyond the
/// 29-bit range the packing admits, and ids beyond the command set.
fn unpack_command_integer(command_integer: u64) -> Result<(u64, u64), VectorTileError> {
    let command_id = command_integer & COMMAND_ID_MASK;
    let count = command_integer >> COMMAND_ID_BIT_COUNT;
    if count == 0 {
        return Err(VectorTileError::GeometryCommandCountOfZero { command_id });
    }
    if count > MAXIMUM_COMMAND_COUNT {
        return Err(VectorTileError::GeometryCommandCountOutOfRange { found: count });
    }
    if command_id != COMMAND_ID_MOVE_TO
        && command_id != COMMAND_ID_LINE_TO
        && command_id != COMMAND_ID_CLOSE_PATH
    {
        return Err(VectorTileError::UnknownGeometryCommand { command_id });
    }
    Ok((count, command_id))
}
