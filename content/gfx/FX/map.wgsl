const SEA_LEVEL: f32 = 0.36;
const LIGHT_DIR: vec3<f32> = vec3<f32>(-0.468293, 0.468293, 0.749269);

@group(1) @binding(0) var norm_map: texture_2d<f32>;
@group(1) @binding(1) var height_map: texture_2d<f32>;
@group(1) @binding(2) var prov_map: texture_2d<u32>;
@group(1) @binding(3) var palette_map: texture_2d<f32>;
@group(1) @binding(4) var smp_linear: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @builtin(instance_index) inst: u32,
) -> VsOut {
    var o: VsOut;
    let world_pos = vec4<f32>(position.x + world_copy_offset(inst), position.y, position.z, 1.0);
    o.pos = frame.view_proj * world_pos;
    o.uv = uv;
    return o;
}

fn province_id(uv: vec2<f32>) -> u32 {
    let dims = vec2<f32>(textureDimensions(prov_map));
    let p = vec2<i32>(clamp(uv * dims, vec2<f32>(0.0), dims - vec2<f32>(1.0)));
    return textureLoad(prov_map, p, 0).r;
}

fn owner_color(id: u32) -> vec4<f32> {
    return textureLoad(palette_map, vec2<i32>(i32(id & 255u), i32(id >> 8u)), 0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let h = textureSample(height_map, smp_linear, in.uv).r;
    let n_raw = textureSample(norm_map, smp_linear, in.uv).rgb * 2.0 - 1.0;
    let n = select(vec3<f32>(0.0, 0.0, 1.0), normalize(n_raw), dot(n_raw, n_raw) > 1e-8);
    let light = max(dot(n, LIGHT_DIR), 0.0) * 0.7 + 0.35;

    if (h < SEA_LEVEL) {
        return vec4<f32>(vec3<f32>(0.05, 0.1, 0.2) * light, 1.0);
    }

    let id = province_id(in.uv);
    let owner = owner_color(id);
    let base = select(vec3<f32>(0.3, 0.35, 0.2), owner.rgb, owner.a > 0.5);
    var color = base * light;

    if (frame.selected.a > 0.5 && id == u32(frame.selected.x)) {
        color += vec3<f32>(0.15, 0.15, 0.0);
    }

    color *= 1.0 - length(n.xy) * 0.3;
    return vec4<f32>(color, 1.0);
}