//! Closed static/untextured GLB subset, before an asset loader sees any bytes.
//! Unknown fields (including URI, extensions, sparse data, skins and animations)
//! reject. All retained arrays and their expanded scene work have fixed budgets.
use crate::{Error, Result};
use flightsim_core::Meters;
use serde::Deserialize;

const MAX_JSON: usize = 1024 * 1024;
const MAX_NODES: usize = 256;
const MAX_MESHES: usize = 256;
const MAX_PRIMITIVES: usize = 512;
const MAX_ACCESSORS: usize = 2048;
const MAX_VERTICES: usize = 500_000;
const MAX_INDICES: usize = 1_500_000;
const MAX_DEPTH: usize = 64;
const MAX_COORDINATE: f64 = 1_000_000.0;

#[derive(Debug, Clone)]
pub struct GeometrySummary {
    /// Actual decoded Scene0 bounds after node transforms, in authored model units.
    pub extents: [Meters; 3],
    pub nodes: usize,
    pub primitives: usize,
    pub vertices: usize,
    pub indices: usize,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Document {
    asset: Asset,
    scene: Option<usize>,
    scenes: Vec<Scene>,
    nodes: Vec<Node>,
    meshes: Vec<Mesh>,
    accessors: Vec<Accessor>,
    buffer_views: Vec<View>,
    buffers: Vec<Buffer>,
    #[serde(default)]
    materials: Vec<Material>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Asset {
    version: String,
    generator: Option<String>,
    copyright: Option<String>,
    min_version: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Scene {
    nodes: Vec<usize>,
    name: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    name: Option<String>,
    mesh: Option<usize>,
    #[serde(default)]
    children: Vec<usize>,
    matrix: Option<[f64; 16]>,
    translation: Option<[f64; 3]>,
    rotation: Option<[f64; 4]>,
    scale: Option<[f64; 3]>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mesh {
    name: Option<String>,
    primitives: Vec<Primitive>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Primitive {
    attributes: Attributes,
    indices: Option<usize>,
    material: Option<usize>,
    mode: Option<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attributes {
    #[serde(rename = "POSITION")]
    position: usize,
    #[serde(rename = "NORMAL")]
    normal: Option<usize>,
    #[serde(rename = "TEXCOORD_0")]
    texcoord: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Accessor {
    buffer_view: usize,
    #[serde(default)]
    byte_offset: usize,
    component_type: u32,
    count: usize,
    #[serde(rename = "type")]
    kind: String,
    min: Option<Vec<f64>>,
    max: Option<Vec<f64>>,
    #[serde(default)]
    normalized: bool,
    name: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct View {
    buffer: usize,
    #[serde(default)]
    byte_offset: usize,
    byte_length: usize,
    byte_stride: Option<usize>,
    target: Option<u32>,
    name: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Buffer {
    byte_length: usize,
    name: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Material {
    name: Option<String>,
    pbr_metallic_roughness: Option<Pbr>,
    emissive_factor: Option<[f64; 3]>,
    alpha_mode: Option<String>,
    alpha_cutoff: Option<f64>,
    double_sided: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Pbr {
    base_color_factor: Option<[f64; 4]>,
    metallic_factor: Option<f64>,
    roughness_factor: Option<f64>,
}

fn invalid(message: &str) -> Error {
    Error::Invalid(format!("aircraft GLB: {message}"))
}
fn word(bytes: &[u8], at: usize) -> Result<usize> {
    let b: [u8; 4] = bytes
        .get(at..at + 4)
        .ok_or_else(|| invalid("truncated header"))?
        .try_into()
        .map_err(|_| invalid("header"))?;
    usize::try_from(u32::from_le_bytes(b)).map_err(|_| invalid("integer overflow"))
}
fn name(value: &Option<String>) -> Result<()> {
    if let Some(s) = value {
        crate::manifest::bounded_text(s, 1024)?;
    }
    Ok(())
}
fn range(values: impl IntoIterator<Item = f64>, low: f64, high: f64) -> Result<()> {
    if values
        .into_iter()
        .any(|v| !v.is_finite() || v < low || v > high)
    {
        return Err(invalid("nonfinite/out-of-domain number"));
    }
    Ok(())
}

pub(super) fn validate(bytes: &[u8]) -> Result<GeometrySummary> {
    if bytes.len() as u64 > super::MAX_GLB_BYTES || bytes.len() < 28 {
        return Err(Error::Limit("aircraft GLB bytes"));
    }
    if &bytes[..4] != b"glTF" || word(bytes, 4)? != 2 || word(bytes, 8)? != bytes.len() {
        return Err(invalid("header/version/length mismatch"));
    }
    let json_len = word(bytes, 12)?;
    if json_len == 0 || json_len > MAX_JSON || json_len % 4 != 0 || &bytes[16..20] != b"JSON" {
        return Err(invalid("JSON chunk format/budget"));
    }
    let json_end = 20usize
        .checked_add(json_len)
        .ok_or_else(|| invalid("chunk overflow"))?;
    let json = bytes
        .get(20..json_end)
        .ok_or_else(|| invalid("truncated JSON"))?;
    let bin_len = word(bytes, json_end)?;
    if bytes.get(json_end + 4..json_end + 8) != Some(b"BIN\0")
        || bin_len % 4 != 0
        || json_end.checked_add(8).and_then(|v| v.checked_add(bin_len)) != Some(bytes.len())
    {
        return Err(invalid(
            "one embedded BIN chunk required; unknown/trailing chunks forbidden",
        ));
    }
    let bin = &bytes[json_end + 8..];
    let doc: Document = serde_json::from_slice(json)?;
    if doc.asset.version != "2.0" || doc.asset.min_version.as_deref().is_some_and(|v| v != "2.0") {
        return Err(invalid("unsupported asset version"));
    }
    name(&doc.asset.generator)?;
    name(&doc.asset.copyright)?;
    if doc.scenes.len() != 1
        || doc.scene.is_some_and(|i| i != 0)
        || doc.scenes[0].nodes.is_empty()
        || doc.scenes[0].nodes.len() > MAX_NODES
        || doc.nodes.is_empty()
        || doc.nodes.len() > MAX_NODES
        || doc.meshes.is_empty()
        || doc.meshes.len() > MAX_MESHES
        || doc.materials.len() > 64
        || doc.accessors.is_empty()
        || doc.accessors.len() > MAX_ACCESSORS
        || doc.buffer_views.len() > MAX_ACCESSORS
        || doc.buffers.len() != 1
    {
        return Err(invalid("scene/resource count budget or missing scene0"));
    }
    name(&doc.scenes[0].name)?;
    name(&doc.buffers[0].name)?;
    let declared = doc.buffers[0].byte_length;
    if declared == 0
        || declared > bin.len()
        || bin.len() - declared > 3
        || bin[declared..].iter().any(|b| *b != 0)
    {
        return Err(invalid("BIN buffer length/padding"));
    }
    let bin = &bin[..declared];
    for view in &doc.buffer_views {
        name(&view.name)?;
        if view.buffer != 0
            || view.byte_length == 0
            || view
                .byte_offset
                .checked_add(view.byte_length)
                .is_none_or(|end| end > bin.len())
            || view
                .byte_stride
                .is_some_and(|n| !(4..=252).contains(&n) || n % 4 != 0)
            || view.target.is_some_and(|v| !matches!(v, 34962 | 34963))
        {
            return Err(invalid("buffer view range/layout"));
        }
    }
    let mut layouts = Vec::with_capacity(doc.accessors.len());
    let mut decoded_scalars = 0usize;
    for accessor in &doc.accessors {
        name(&accessor.name)?;
        let components = match accessor.kind.as_str() {
            "SCALAR" => 1,
            "VEC2" => 2,
            "VEC3" => 3,
            _ => return Err(invalid("unsupported accessor type")),
        };
        let component_bytes = match accessor.component_type {
            5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(invalid("unsupported accessor component")),
        };
        if accessor.normalized
            || accessor.count == 0
            || accessor.count > MAX_INDICES
            || (components != 1 && accessor.component_type != 5126)
        {
            return Err(invalid("unsupported/over-budget accessor"));
        }
        decoded_scalars = decoded_scalars.saturating_add(accessor.count.saturating_mul(components));
        if decoded_scalars > MAX_VERTICES * 8 + MAX_INDICES {
            return Err(Error::Limit("GLB decoded scalar work"));
        }
        let view = doc
            .buffer_views
            .get(accessor.buffer_view)
            .ok_or_else(|| invalid("accessor view index"))?;
        let width = components * component_bytes;
        let stride = view.byte_stride.unwrap_or(width);
        if stride < width
            || stride % component_bytes != 0
            || accessor.byte_offset % component_bytes != 0
            || view.byte_offset % component_bytes != 0
            || accessor
                .byte_offset
                .checked_add((accessor.count - 1).saturating_mul(stride))
                .and_then(|v| v.checked_add(width))
                .is_none_or(|end| end > view.byte_length)
        {
            return Err(invalid("accessor data range/alignment"));
        }
        let layout = Layout {
            offset: view.byte_offset + accessor.byte_offset,
            stride,
            components,
            component_type: accessor.component_type,
            count: accessor.count,
        };
        let mut minima = vec![f64::INFINITY; components];
        let mut maxima = vec![f64::NEG_INFINITY; components];
        for i in 0..accessor.count {
            for c in 0..components {
                let value = layout.value(bin, i, c);
                if !value.is_finite()
                    || accessor.component_type == 5126 && value.abs() > MAX_COORDINATE
                {
                    return Err(invalid("nonfinite/oversized binary float"));
                }
                minima[c] = minima[c].min(value);
                maxima[c] = maxima[c].max(value);
            }
        }
        for (claimed, actual) in [(&accessor.min, &minima), (&accessor.max, &maxima)] {
            if let Some(values) = claimed {
                if values.len() != components
                    || values
                        .iter()
                        .zip(actual)
                        .any(|(a, b)| !a.is_finite() || (a - b).abs() > 1e-6 * b.abs().max(1.0))
                {
                    return Err(invalid("accessor min/max disagrees with binary data"));
                }
            }
        }
        layouts.push(layout);
    }
    for material in &doc.materials {
        name(&material.name)?;
        if let Some(pbr) = &material.pbr_metallic_roughness {
            range(pbr.base_color_factor.unwrap_or([1.0; 4]), 0.0, 1.0)?;
            range(
                [
                    pbr.metallic_factor.unwrap_or(1.0),
                    pbr.roughness_factor.unwrap_or(1.0),
                ],
                0.0,
                1.0,
            )?;
        }
        range(material.emissive_factor.unwrap_or([0.0; 3]), 0.0, 1.0)?;
        range([material.alpha_cutoff.unwrap_or(0.5)], 0.0, 1.0)?;
        if material
            .alpha_mode
            .as_deref()
            .is_some_and(|s| !matches!(s, "OPAQUE" | "MASK" | "BLEND"))
        {
            return Err(invalid("unsupported alpha mode"));
        }
        let _ = material.double_sided;
    }
    let mut primitives = 0usize;
    let mut mesh_vertices = vec![];
    let mut mesh_indices = vec![];
    let mut source_vertices = 0usize;
    let mut source_indices = 0usize;
    for mesh in &doc.meshes {
        name(&mesh.name)?;
        if mesh.primitives.is_empty() {
            return Err(invalid("empty mesh"));
        }
        primitives = primitives.saturating_add(mesh.primitives.len());
        if primitives > MAX_PRIMITIVES {
            return Err(Error::Limit("GLB primitive count"));
        }
        let mut vertices = 0;
        let mut indices = 0;
        for p in &mesh.primitives {
            if p.mode.is_some_and(|m| m != 4)
                || p.material.is_some_and(|m| m >= doc.materials.len())
            {
                return Err(invalid("triangles/material index required"));
            }
            let positions = layouts
                .get(p.attributes.position)
                .ok_or_else(|| invalid("POSITION index"))?;
            if positions.components != 3 || positions.component_type != 5126 {
                return Err(invalid("POSITION must be f32 VEC3"));
            }
            let position_accessor = &doc.accessors[p.attributes.position];
            if position_accessor.min.is_none() || position_accessor.max.is_none() {
                return Err(invalid("POSITION requires actual min/max"));
            }
            for (index, components) in [(p.attributes.normal, 3), (p.attributes.texcoord, 2)] {
                if let Some(index) = index {
                    let a = layouts
                        .get(index)
                        .ok_or_else(|| invalid("attribute index"))?;
                    if a.component_type != 5126
                        || a.components != components
                        || a.count != positions.count
                    {
                        return Err(invalid("attribute layout/count mismatch"));
                    }
                    if components == 3 {
                        for i in 0..a.count {
                            let n = a.vector(bin, i);
                            let length = n.iter().map(|v| v * v).sum::<f64>();
                            if !(0.99..=1.01).contains(&length) {
                                return Err(invalid("NORMAL must be unit length"));
                            }
                        }
                    }
                }
            }
            let count = if let Some(index) = p.indices {
                let a = layouts.get(index).ok_or_else(|| invalid("indices index"))?;
                if a.components != 1
                    || !matches!(a.component_type, 5123 | 5125)
                    || doc.buffer_views[doc.accessors[index].buffer_view]
                        .byte_stride
                        .is_some()
                {
                    return Err(invalid("indices must be packed unsigned scalars"));
                }
                for i in 0..a.count {
                    if a.value(bin, i, 0)
                        >= f64::from(u32::try_from(positions.count).expect("bounded vertex count"))
                    {
                        return Err(invalid("triangle index outside POSITION"));
                    }
                }
                a.count
            } else {
                positions.count
            };
            if count % 3 != 0 {
                return Err(invalid("incomplete triangles"));
            }
            let vertex = |i: usize| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let index = p
                    .indices
                    .map_or(i, |a| layouts[a].value(bin, i, 0) as usize);
                positions.vector(bin, index)
            };
            let has_surface = (0..count).step_by(3).any(|i| {
                let a = vertex(i);
                let b = vertex(i + 1);
                let c = vertex(i + 2);
                let u: [f64; 3] = std::array::from_fn(|n| b[n] - a[n]);
                let v: [f64; 3] = std::array::from_fn(|n| c[n] - a[n]);
                let cross = [
                    u[1] * v[2] - u[2] * v[1],
                    u[2] * v[0] - u[0] * v[2],
                    u[0] * v[1] - u[1] * v[0],
                ];
                cross.iter().map(|n| n * n).sum::<f64>() > 1e-24
            });
            if !has_surface {
                return Err(invalid("primitive has no nondegenerate triangle"));
            }
            vertices += positions.count;
            indices += count;
        }
        source_vertices += vertices;
        source_indices += indices;
        if source_vertices > MAX_VERTICES || source_indices > MAX_INDICES {
            return Err(Error::Limit("GLB mesh work"));
        }
        mesh_vertices.push(vertices);
        mesh_indices.push(indices);
    }
    let mut parents = vec![0u8; doc.nodes.len()];
    let mut transforms = Vec::with_capacity(doc.nodes.len());
    for node in &doc.nodes {
        name(&node.name)?;
        if node.mesh.is_some_and(|m| m >= doc.meshes.len()) || node.children.len() > MAX_NODES {
            return Err(invalid("node mesh/children budget"));
        }
        for child in &node.children {
            let n = parents
                .get_mut(*child)
                .ok_or_else(|| invalid("node child index"))?;
            *n = n.saturating_add(1);
            if *n > 1 {
                return Err(invalid("node has duplicate/multiple parents"));
            }
        }
        transforms.push(transform(node)?);
    }
    let mut seen = vec![false; doc.nodes.len()];
    let mut stack = vec![];
    let mut roots = std::collections::BTreeSet::new();
    for root in &doc.scenes[0].nodes {
        if !roots.insert(*root) || parents.get(*root) != Some(&0) {
            return Err(invalid("scene root index/parent"));
        }
        stack.push((*root, identity(), 0usize));
    }
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    let mut expanded_vertices = 0usize;
    let mut expanded_indices = 0usize;
    let mut expanded_primitives = 0usize;
    while let Some((index, parent, depth)) = stack.pop() {
        if depth > MAX_DEPTH || seen[index] {
            return Err(invalid("node cycle/duplicate/depth"));
        }
        seen[index] = true;
        let node = &doc.nodes[index];
        let world = multiply(parent, transforms[index]);
        check_matrix(&world)?;
        if let Some(mesh) = node.mesh {
            expanded_vertices = expanded_vertices.saturating_add(mesh_vertices[mesh]);
            expanded_indices = expanded_indices.saturating_add(mesh_indices[mesh]);
            expanded_primitives += doc.meshes[mesh].primitives.len();
            if expanded_vertices > MAX_VERTICES
                || expanded_indices > MAX_INDICES
                || expanded_primitives > MAX_PRIMITIVES
            {
                return Err(Error::Limit("GLB expanded scene work"));
            }
            for p in &doc.meshes[mesh].primitives {
                let positions = &layouts[p.attributes.position];
                for i in 0..positions.count {
                    let pos = positions.vector(bin, i);
                    for c in 0..3 {
                        let v = world[c] * pos[0]
                            + world[4 + c] * pos[1]
                            + world[8 + c] * pos[2]
                            + world[12 + c];
                        if !v.is_finite() || v.abs() > MAX_COORDINATE {
                            return Err(invalid("transformed vertex outside domain"));
                        }
                        low[c] = low[c].min(v);
                        high[c] = high[c].max(v);
                    }
                }
            }
        }
        for child in &node.children {
            stack.push((*child, world, depth + 1));
        }
    }
    if seen.iter().any(|v| !*v) || expanded_vertices == 0 {
        return Err(invalid("unreachable/cyclic nodes or empty Scene0"));
    }
    let extents = std::array::from_fn(|i| Meters(high[i] - low[i]));
    if extents.iter().any(|v| !v.get().is_finite()) || extents.iter().all(|v| v.get() < 1e-6) {
        return Err(invalid("degenerate Scene0 bounds"));
    }
    Ok(GeometrySummary {
        extents,
        nodes: doc.nodes.len(),
        primitives: expanded_primitives,
        vertices: expanded_vertices,
        indices: expanded_indices,
    })
}
struct Layout {
    offset: usize,
    stride: usize,
    components: usize,
    component_type: u32,
    count: usize,
}
impl Layout {
    // Layout was proved in-bounds before these fixed-size reads.
    fn value(&self, bin: &[u8], index: usize, component: usize) -> f64 {
        let width = if self.component_type == 5123 { 2 } else { 4 };
        let at = self.offset + index * self.stride + component * width;
        match self.component_type {
            5123 => f64::from(u16::from_le_bytes(
                bin[at..at + 2].try_into().expect("bounded u16"),
            )),
            5125 => f64::from(u32::from_le_bytes(
                bin[at..at + 4].try_into().expect("bounded u32"),
            )),
            _ => f64::from(f32::from_le_bytes(
                bin[at..at + 4].try_into().expect("bounded float"),
            )),
        }
    }
    fn vector(&self, bin: &[u8], index: usize) -> [f64; 3] {
        std::array::from_fn(|i| self.value(bin, index, i))
    }
}
fn identity() -> [f64; 16] {
    [
        1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
    ]
}
fn multiply(a: [f64; 16], b: [f64; 16]) -> [f64; 16] {
    std::array::from_fn(|i| (0..4).map(|k| a[k * 4 + i % 4] * b[i / 4 * 4 + k]).sum())
}
#[allow(
    clippy::float_cmp,
    reason = "affine homogeneous row is an exact closed format constraint"
)]
fn check_matrix(m: &[f64; 16]) -> Result<()> {
    range(*m, -MAX_COORDINATE, MAX_COORDINATE)?;
    if m[3] != 0.0 || m[7] != 0.0 || m[11] != 0.0 || m[15] != 1.0 {
        return Err(invalid("non-affine matrix"));
    }
    let determinant = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
        + m[8] * (m[1] * m[6] - m[5] * m[2]);
    if determinant.abs() < 1e-12 {
        return Err(invalid("singular transform"));
    }
    Ok(())
}
fn transform(n: &Node) -> Result<[f64; 16]> {
    // Bevy decomposes matrices into TRS. v1 accepts authored TRS only so shear
    // or decomposition conventions cannot change the validated geometry.
    if n.matrix.is_some() {
        return Err(invalid("matrix transforms unsupported; use explicit TRS"));
    }
    let [x, y, z, w] = n.rotation.unwrap_or([0., 0., 0., 1.]);
    range([x, y, z, w], -1., 1.)?;
    if (x * x + y * y + z * z + w * w - 1.).abs() > 1e-5 {
        return Err(invalid("non-unit quaternion"));
    }
    let t = n.translation.unwrap_or([0.; 3]);
    let s = n.scale.unwrap_or([1.; 3]);
    let mut m = [
        1. - 2. * (y * y + z * z),
        2. * (x * y + z * w),
        2. * (x * z - y * w),
        0.,
        2. * (x * y - z * w),
        1. - 2. * (x * x + z * z),
        2. * (y * z + x * w),
        0.,
        2. * (x * z + y * w),
        2. * (y * z - x * w),
        1. - 2. * (x * x + y * y),
        0.,
        t[0],
        t[1],
        t[2],
        1.,
    ];
    for c in 0..3 {
        for r in 0..3 {
            m[c * 4 + r] *= s[c];
        }
    }
    check_matrix(&m)?;
    Ok(m)
}
