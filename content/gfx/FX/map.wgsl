// Порт Map.vert / Map.frag (GLSL 410) один к одному.
const SEA_LEVEL: f32 = 0.36;

@group(1) @binding(0) var norm_map: texture_2d<f32>;
@group(1) @binding(1) var height_map: texture_2d<f32>;
@group(1) @binding(2) var prov_map: texture_2d<f32>;
@group(1) @binding(3) var state_map: texture_2d<f32>;
@group(1) @binding(4) var smp_linear: sampler;
@group(1) @binding(5) var smp_nearest: sampler;

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
    // m = translate(offset, 0, 0) для копий мира -1/0/+1
    let world_pos = vec4<f32>(position.x + world_copy_offset(inst), position.y, position.z, 1.0);
    o.pos = frame.view_proj * world_pos;
    o.uv = uv;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;

    // все выборки — до ветвлений (uniform control flow)
    let h = textureSample(height_map, smp_linear, uv).r;
    let n_raw = textureSample(norm_map, smp_linear, uv).rgb * 2.0 - 1.0;
    let prov_color = textureSampleLevel(prov_map, smp_nearest, uv, 0.0).rgb;
    let state_color = textureSampleLevel(state_map, smp_nearest, uv, 0.0).rgb;

    let n = select(vec3<f32>(0.0, 0.0, 1.0), normalize(n_raw), dot(n_raw, n_raw) > 1e-8);

    let land_mask = step(SEA_LEVEL, h);
    let light = max(dot(n, normalize(vec3<f32>(-0.5, 0.5, 0.8))), 0.0) * 0.7 + 0.35;

    var final_color: vec3<f32>;
    if (land_mask > 0.5) {
        let base = select(vec3<f32>(0.3, 0.35, 0.2), state_color, length(state_color) > 0.01);
        final_color = base * light;

        // подсветка выбранной провинции
        if (frame.selected.a > 0.5 && distance(prov_color, frame.selected.rgb) < 0.001) {
            final_color += vec3<f32>(0.15, 0.15, 0.0);
        }

        final_color *= (1.0 - length(n.xy) * 0.3);
    } else {
        final_color = vec3<f32>(0.05, 0.1, 0.2) * light;
    }

    return vec4<f32>(final_color, 1.0);
}
