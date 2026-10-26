//! Triangle geometry shaders lowered to bounded compute output and a vertex reader.

use crate::compile::{CompiledStage, OutputMap, compile};
use crate::diagnostics::{Diagnostic, Phase};
use crate::preprocess::{Expanded, LineMap, Preprocessed, preprocess};
use crate::programs::StageKind;
use crate::token::{Tok, id, lex};
use crate::transform::items::{layout_key, parse_decl, split_items};

/// The host-owned storage group used only by emulated geometry programs.
pub const GEOMETRY_GROUP: u32 = crate::reflection::STORAGE_BUFFER_GROUP;
/// Storage bindings reserved by geometry emulation, in binding order.
pub const GEOMETRY_BINDINGS: u32 = 7;

/// One scalar or vector passed between the emulated stages, in 32-bit words.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GeometryField {
    pub name: String,
    pub ty: String,
    /// Explicit stage interface location; implicit interfaces link by name.
    pub location: Option<u32>,
    pub offset: u32,
    pub words: u32,
}

/// The bounded output contract for one input triangle.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GeometryEmulation {
    pub max_vertices: u32,
    pub input_words: u32,
    pub output_words: u32,
    pub inputs: Vec<GeometryField>,
    pub outputs: Vec<GeometryField>,
    /// The vertex stage that reads the compute output. Clip conversion happens here.
    pub vertex: CompiledStage,
}

/// Prepared sources before the program's shared uniforms and bindings are assigned.
pub(crate) struct LoweredGeometry {
    pub max_vertices: u32,
    pub input_words: u32,
    pub output_words: u32,
    pub inputs: Vec<GeometryField>,
    pub outputs: Vec<GeometryField>,
    pub compute: Preprocessed,
    pub vertex: Preprocessed,
}

/// Returns the number of words and scalar storage conversion for a supported varying.
fn field_words(ty: &str) -> Option<u32> {
    match ty {
        "float" | "int" | "uint" => Some(1),
        "vec2" | "ivec2" | "uvec2" => Some(2),
        "vec3" | "ivec3" | "uvec3" => Some(3),
        "vec4" | "ivec4" | "uvec4" => Some(4),
        _ => None,
    }
}

/// Encodes one field into consecutive words without std430 padding.
fn store(field: &GeometryField, value: &str, buffer: &str, base: &str) -> String {
    (0..field.words)
        .map(|i| {
            let value = if field.words == 1 {
                value.to_owned()
            } else {
                format!("{value}[{i}]")
            };
            let bits = if field.ty.starts_with('i') || field.ty.starts_with('u') {
                format!("uint({value})")
            } else {
                format!("floatBitsToUint({value})")
            };
            format!("{buffer}[{base} + {}u] = {bits};\n", field.offset + i)
        })
        .collect()
}

/// Decodes one scalar or vector from consecutive words.
fn load(field: &GeometryField, buffer: &str, base: &str) -> String {
    let scalar = if field.ty.starts_with('i') {
        "int"
    } else if field.ty.starts_with('u') {
        "uint"
    } else {
        "uintBitsToFloat"
    };
    let values = (0..field.words)
        .map(|i| format!("{scalar}({buffer}[{base} + {}u])", field.offset + i))
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({values})", field.ty)
}

/// Clones source metadata while replacing the fully preprocessed token stream.
fn source(pp: &Preprocessed, tokens: Vec<Tok>) -> Preprocessed {
    Preprocessed {
        version: 450,
        profile: Some("core".into()),
        extensions: pp.extensions.clone(),
        tokens,
        map: pp.map.clone(),
        render_targets: None,
        consts: vec![],
    }
}

/// Reads a complete literal layout value, rejecting expressions instead of truncating them.
fn literal_layout(layout: &[Tok], key: &str) -> Result<Option<u32>, String> {
    let Some(index) = layout.iter().position(|token| token.ident() == Some(key)) else {
        return Ok(None);
    };
    if layout.get(index + 3).is_none_or(|token| token.is(","))
        && let Some(value) = layout_key(layout, key)
    {
        return Ok(Some(value));
    }
    Err(format!(
        "geometry layout {key} requires a literal unsigned integer"
    ))
}

/// Lowers triangle input and triangle-strip emission; unsupported contracts fail this program.
pub(crate) fn lower(pp: &Preprocessed) -> Result<LoweredGeometry, String> {
    let unsupported = [
        "gl_Layer",
        "gl_ViewportIndex",
        "gl_ClipDistance",
        "gl_CullDistance",
        "gl_PointSize",
        "EmitStreamVertex",
        "EndStreamPrimitive",
    ];
    if let Some(name) = unsupported
        .iter()
        .find(|name| pp.tokens.iter().any(|t| t.ident() == Some(name)))
    {
        return Err(format!("geometry output {name} cannot be emulated"));
    }
    let mut inputs = vec![];
    let mut outputs = vec![];
    let (mut input_words, mut output_words) = (4u32, 4u32);
    let mut body = vec![];
    let mut triangles = false;
    let mut triangle_strip = false;
    let mut max_vertices = None;
    for item in split_items(&pp.tokens) {
        if let Some(d) = parse_decl(&item) {
            if d.has("buffer") {
                return Err(
                    "geometry shaders using pack storage buffers cannot be emulated".into(),
                );
            }
            if d.ty.is_empty() && d.has("in") {
                triangles |= d.layout.iter().any(|t| t.ident() == Some("triangles"));
                if literal_layout(&d.layout, "invocations")?.is_some_and(|n| n != 1) {
                    return Err("multiple geometry invocations cannot be emulated".into());
                }
                continue;
            }
            if d.ty.is_empty() && d.has("out") {
                if d.layout
                    .iter()
                    .any(|t| matches!(t.ident(), Some("points" | "line_strip")))
                {
                    return Err("geometry emulation requires triangle_strip output".into());
                }
                triangle_strip |= d.layout.iter().any(|t| t.ident() == Some("triangle_strip"));
                if let Some(count) = literal_layout(&d.layout, "max_vertices")? {
                    if max_vertices.is_some_and(|old| old != count) {
                        return Err("geometry output has conflicting max_vertices".into());
                    }
                    max_vertices = Some(count);
                }
                continue;
            }
            if d.has("in") || d.has("out") {
                if d.block.is_some() || !d.ty_arr.is_empty() {
                    return Err(
                        "geometry interface blocks and nested arrays cannot be emulated".into(),
                    );
                }
                let words = field_words(&d.ty)
                    .ok_or_else(|| format!("geometry varying type {} cannot be emulated", d.ty))?;
                let mut location = literal_layout(&d.layout, "location")?;
                for (name, array, init) in &d.decls {
                    let interface_name = crate::transform::interface_identifier(name, pp.version)
                        .map_err(|error| error.message)?;
                    if !init.is_empty() || d.has("out") && !array.is_empty() {
                        return Err(format!(
                            "geometry varying {name} has an unsupported declaration"
                        ));
                    }
                    if d.has("in") {
                        if array.is_empty() {
                            return Err(format!("geometry input {name} must be an array"));
                        }
                        inputs.push(GeometryField {
                            name: interface_name,
                            ty: d.ty.clone(),
                            location,
                            offset: input_words,
                            words,
                        });
                        input_words = input_words
                            .checked_add(words)
                            .ok_or("geometry input stride overflow")?;
                        body.extend(lex(&format!("{} {name}[3];", d.ty)));
                    } else {
                        outputs.push(GeometryField {
                            name: interface_name,
                            ty: d.ty.clone(),
                            location,
                            offset: output_words,
                            words,
                        });
                        output_words = output_words
                            .checked_add(words)
                            .ok_or("geometry output stride overflow")?;
                        body.extend(lex(&format!("{} {name};", d.ty)));
                    }
                    location = location
                        .map(|value| {
                            value
                                .checked_add(1)
                                .ok_or("geometry varying location overflow")
                        })
                        .transpose()?;
                }
                continue;
            }
        }
        body.extend(item);
    }
    if !triangles {
        return Err("geometry emulation requires triangles input".into());
    }
    if !triangle_strip {
        return Err("geometry emulation requires triangle_strip output".into());
    }
    let max_vertices = max_vertices
        .filter(|&n| n >= 3)
        .ok_or("geometry shader needs max_vertices of at least three")?;
    max_vertices
        .checked_mul(output_words)
        .and_then(|_| (max_vertices - 2).checked_mul(3))
        .ok_or("geometry output size overflow")?;
    for token in &mut body {
        if let Some(name) = token.ident() {
            let replacement = match name {
                "main" => Some("cb_geometry_main"),
                "EmitVertex" => Some("cb_emit"),
                "EndPrimitive" => Some("cb_end"),
                "gl_Position" => Some("cb_position"),
                "gl_in" => Some("cb_vertices_in"),
                "gl_PrimitiveIDIn" => Some("cb_primitive_id"),
                "gl_InvocationID" => Some("cb_invocation"),
                _ => None,
            };
            if let Some(name) = replacement {
                *token = id(name).at(token.line);
            }
        }
    }
    let position = GeometryField {
        name: "cb_position".into(),
        ty: "vec4".into(),
        location: None,
        offset: 0,
        words: 4,
    };
    let mut header = String::from(
        "layout(local_size_x=64) in;\nlayout(binding=3) readonly buffer cb_Input { uint cb_input[]; };\nlayout(binding=4) buffer cb_Output { uint cb_output[]; };\nlayout(binding=5) buffer cb_Indices { uint cb_indices[]; };\nlayout(binding=6) readonly buffer cb_Params { uint cb_params[]; };\nstruct cb_PerVertex { vec4 cb_position; };\ncb_PerVertex cb_vertices_in[3];\nvec4 cb_position; uint cb_primitive; int cb_primitive_id; int cb_invocation; uint cb_emitted; uint cb_strip; uint cb_triangles;\n",
    );
    header.push_str("void cb_emit(); void cb_end();\n");
    let mut emit = format!(
        "void cb_emit() {{ if(cb_emitted >= {max_vertices}u) return; uint base = (cb_primitive*{max_vertices}u+cb_emitted)*{output_words}u;\n"
    );
    emit.push_str(&store(&position, "cb_position", "cb_output", "base"));
    for field in &outputs {
        emit.push_str(&store(field, &field.name, "cb_output", "base"));
    }
    emit.push_str(&format!("if(cb_strip>=2u) {{ uint index = (cb_primitive*{}u+cb_triangles)*3u; uint v=cb_primitive*{max_vertices}u+cb_emitted; cb_indices[index]=v; cb_indices[index+1u]=v-2u+(cb_strip%2u); cb_indices[index+2u]=v-1u-(cb_strip%2u); cb_triangles++; }} cb_strip++; cb_emitted++; }}\nvoid cb_end() {{ cb_strip=0u; }}\n",max_vertices-2));
    // Outputs must be declared before the emission helper accesses them.
    let mut tokens = lex(&header);
    tokens.extend(body);
    tokens.extend(lex(&emit));
    header = format!(
        "void main() {{ cb_primitive=gl_GlobalInvocationID.x; if(cb_primitive>=cb_params[5]) return; cb_primitive_id=int(cb_params[4]+cb_primitive); cb_invocation=0; cb_emitted=0u; cb_strip=0u; cb_triangles=0u; for(uint j=0u;j<{}u;j++) cb_indices[cb_primitive*{}u+j]=cb_primitive*{max_vertices}u; for(uint i=0u;i<3u;i++) {{ uint base=(cb_primitive*3u+i)*{input_words}u; cb_vertices_in[i].cb_position={};",
        (max_vertices - 2) * 3,
        (max_vertices - 2) * 3,
        load(&position, "cb_input", "base")
    );
    for field in &inputs {
        header.push_str(&format!(
            "{}[i]={};",
            field.name,
            load(field, "cb_input", "base")
        ));
    }
    header.push_str("} cb_geometry_main(); }");
    tokens.extend(lex(&header));
    let mut vertex =
        String::from("layout(binding=4) readonly buffer cb_Output {uint cb_output[];};\n");
    for item in split_items(&pp.tokens) {
        if let Some(d) = parse_decl(&item)
            && d.has("out")
            && !d.ty.is_empty()
        {
            vertex.push_str(&item.iter().map(Tok::text).collect::<Vec<_>>().join(" "));
        }
    }
    vertex.push_str(&format!(
        "void main() {{ uint base=uint(gl_VertexID)*{output_words}u; gl_Position={};",
        load(&position, "cb_output", "base")
    ));
    for field in &outputs {
        vertex.push_str(&format!(
            "{}={};",
            field.name,
            load(field, "cb_output", "base")
        ));
    }
    vertex.push('}');
    Ok(LoweredGeometry {
        max_vertices,
        input_words,
        output_words,
        inputs,
        outputs,
        compute: source(pp, tokens),
        vertex: source(pp, lex(&vertex)),
    })
}

/// Compiles a generated vertex pre-pass with host-specific attribute decoding.
/// `vertex` must be this geometry program's vertex stage, which retains OpenGL clip space.
/// `loads` supplies expressions for live vertex input locations from the host's storage streams.
pub fn geometry_vertex_compute(
    vertex: &CompiledStage,
    geometry: &GeometryEmulation,
    loads: &[(u32, String)],
) -> Result<CompiledStage, Vec<Diagnostic>> {
    let translated = preprocess(
        Expanded {
            text: vertex.glsl.clone(),
            map: LineMap::default(),
        },
        &[],
    )
    .map_err(|error| vec![error])?;
    let mut tokens = Vec::new();
    let mut assignments = String::new();
    let mut outputs = Vec::new();
    for item in split_items(&translated.tokens) {
        if let Some(d) = parse_decl(&item)
            && (d.has("in") || d.has("out"))
        {
            if d.block.is_some() {
                return Err(vec![Diagnostic::new(
                    Phase::Transform,
                    "vertex interface blocks cannot be emulated",
                )]);
            }
            for (name, arr, _) in &d.decls {
                let location = layout_key(&d.layout, "location");
                if d.has("in") {
                    if let Some((_, value)) =
                        loads.iter().find(|(input, _)| Some(*input) == location)
                    {
                        assignments.push_str(&format!("{name}={value};"));
                    }
                } else {
                    outputs.push((name.clone(), location, d.ty.clone()));
                }
                tokens.extend(lex(&format!(
                    "{} {name} {} ;",
                    d.ty,
                    arr.iter().map(Tok::text).collect::<Vec<_>>().join(" ")
                )));
            }
        } else {
            tokens.extend(item);
        }
    }
    for token in &mut tokens {
        let replacement = match token.ident() {
            Some("main") => Some("cb_vertex_main"),
            Some("gl_Position") => Some("cb_position"),
            Some("gl_VertexIndex") => Some("cb_vertex_id"),
            Some("gl_InstanceIndex") => Some("cb_instance_id"),
            _ => None,
        };
        if let Some(name) = replacement {
            *token = id(name).at(token.line);
        }
    }
    let body = tokens.iter().map(Tok::text).collect::<Vec<_>>().join(" ");
    let mut glsl = format!(
        "#version 450\nlayout(local_size_x=64) in;\nlayout(set=3,binding=0) readonly buffer cb_Vertices {{uint cb_vertices[];}};\nlayout(set=3,binding=1) readonly buffer cb_Elements {{uint cb_elements[];}};\nlayout(set=3,binding=2) readonly buffer cb_Origins {{uint cb_origins[];}};\nlayout(set=3,binding=3) buffer cb_Input {{uint cb_input[];}};\nlayout(set=3,binding=6) readonly buffer cb_Params {{uint cb_params[];}};\nvec4 cb_position; int cb_vertex_id; int cb_instance_id;\n{body}\nvoid main() {{ uint invocation=gl_GlobalInvocationID.x; if(invocation>=cb_params[5]*3u) return; uint element=cb_params[0]+invocation; cb_vertex_id=int(cb_params[1]==0u?element:cb_elements[element])+int(cb_params[2]); cb_instance_id=int(cb_params[3]); {assignments} cb_vertex_main(); uint base=invocation*{}u;",
        geometry.input_words
    );
    let position = GeometryField {
        name: "cb_position".into(),
        ty: "vec4".into(),
        location: None,
        offset: 0,
        words: 4,
    };
    glsl.push_str(&store(&position, "cb_position", "cb_input", "base"));
    for field in &geometry.inputs {
        let producer = outputs
            .iter()
            .find(|(name, location, _)| {
                field
                    .location
                    .map_or(name == &field.name, |input| *location == Some(input))
            })
            .filter(|(_, _, ty)| *ty == field.ty)
            .ok_or_else(|| {
                vec![Diagnostic::new(
                    Phase::Transform,
                    format!(
                        "geometry input {} has no compatible vertex output",
                        field.name
                    ),
                )]
            })?;
        glsl.push_str(&store(field, &producer.0, "cb_input", "base"));
    }
    glsl.push('}');
    compile(
        StageKind::Compute,
        &OutputMap {
            glsl: &glsl,
            origins: &[],
            lines: &LineMap::default(),
        },
    )
    .map_err(|e| e.diagnostics)
}
