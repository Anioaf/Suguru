// Порт Text.vert / Text.frag (GLSL 410).
const TEXT_COLOR = vec3<f32>(1.0, 1.0, 1.0);

@group(1) @binding(0) var text_sampler_tex: texture_2d<f32>;
@group(1) @binding(1) var text_sampler: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) vertex: vec4<f32>, @builtin(instance_index) inst: u32) -> VsOut {
    var o: VsOut;
    o.pos = frame.view_proj * vec4<f32>(vertex.x + world_copy_offset(inst), vertex.y, 0.0, 1.0);
    o.uv = vertex.zw;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let alpha = textureSample(text_sampler_tex, text_sampler, in.uv).r;
    if (alpha < 0.1) {
        discard;
    }
    return vec4<f32>(TEXT_COLOR, alpha);
}
