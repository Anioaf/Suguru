// Аналог constants.fxh: подставляется перед КАЖДЫМ шейдером.
struct FrameUniform {
    view_proj: mat4x4<f32>,
    selected: vec4<f32>,   // rgb выбранной провинции, a = 1 если что-то выбрано
    params: vec4<f32>,     // x = время, y = ширина мира
};
@group(0) @binding(0) var<uniform> frame: FrameUniform;

// Три копии мира (-1, 0, +1) рисуются одним instanced draw-call'ом.
fn world_copy_offset(instance: u32) -> f32 {
    return (f32(instance) - 1.0) * frame.params.y;
}
