// World material particles: `count` pixel squares around each bound target, which the
// material's `particle` function places within its radius and colours. Each instance is one
// particle of one target; the four vertices of a square face the camera.

struct ParticleView {
    clip_from_world: mat4x4<f32>,
    // The camera's right and up directions in the world.
    right: vec4<f32>,
    up: vec4<f32>,
}
@group(0) @binding(0) var<uniform> particle_view: ParticleView;
// Each target's position in the world.
@group(0) @binding(1) var<storage, read> particle_origins: array<vec4<f32>>;

struct ParticleVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn particle_vertex(
    @builtin(vertex_index) vertex: u32,
    @builtin(instance_index) instance: u32,
) -> ParticleVertex {
    let count = max(u32(world_material_frame.effects[0].z), 1u);
    let origin = particle_origins[min(instance / count, arrayLength(&particle_origins) - 1u)].xyz;
    let particle = world_material_particle(
        WorldParticleInput(instance % count, count, world_material_phase(), origin),
    );
    var out: ParticleVertex;
    if particle.size <= 0.0 || particle.coverage < 0.5 {
        out.position = vec4(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let corner = vec2(f32(vertex & 1u), f32((vertex >> 1u) & 1u)) - vec2(0.5);
    let side = particle.size / 16.0;
    let world = origin + particle.offset
        + (particle_view.right.xyz * corner.x + particle_view.up.xyz * corner.y) * side;
    out.position = particle_view.clip_from_world * vec4(world, 1.0);
    out.colour = vec4(particle.color, 1.0);
    return out;
}

// Pixel particles draw as solid emissive squares.
@fragment
fn particle_fragment(input: ParticleVertex) -> @location(0) vec4<f32> {
    return input.colour;
}
