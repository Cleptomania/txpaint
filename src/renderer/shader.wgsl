struct Uniforms {
    // Canvas cell-space coordinate at (uv = (0,0)) — i.e. the canvas cell at the
    // top-left of the current viewport. Fractional values represent sub-cell
    // panning.
    cell_origin: vec2<f32>,
    // Cell-space span across (uv = (0,0)) → (uv = (1,1)).
    cell_span: vec2<f32>,
    canvas_wh: vec2<u32>,
    _pad0: vec2<u32>,
};

struct LayerUniforms {
    // Canvas-space cell at the layer's buffer origin. Buffer cell =
    // canvas_cell - offset. Layers can be positioned partly or fully off the
    // canvas; discard below handles both the canvas viewport and layer bounds.
    offset: vec2<i32>,
    // Layer buffer size in cells. Used for the per-layer bounds discard so
    // layers smaller than the canvas draw as transparent outside their buffer.
    layer_wh: vec2<u32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var font_tex: texture_2d<f32>;
@group(0) @binding(2) var font_samp: sampler;

@group(1) @binding(0) var glyph_tex: texture_2d<u32>;
@group(1) @binding(1) var fg_tex: texture_2d<f32>;
@group(1) @binding(2) var bg_tex: texture_2d<f32>;
@group(1) @binding(3) var<uniform> layer: LayerUniforms;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Full-viewport quad. egui-wgpu sets the render pass viewport to the
    // paint callback rect, so NDC (-1..1, -1..1) fills exactly the canvas
    // drawing region. UV uses y-down (0,0 = top-left, 1,1 = bottom-right)
    // to match texture storage convention.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let ndc = vec2<f32>(c.x * 2.0 - 1.0, 1.0 - c.y * 2.0);
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let canvas_fpos = u.cell_origin + in.uv * u.cell_span;
    let canvas_wh = vec2<f32>(u.canvas_wh);
    // Reject fragments outside the canvas bounds — happens when the view is
    // panned past the canvas edge, or for layer content that extends past it.
    if canvas_fpos.x < 0.0 || canvas_fpos.y < 0.0
        || canvas_fpos.x >= canvas_wh.x || canvas_fpos.y >= canvas_wh.y {
        discard;
    }
    // Translate canvas-space cell to layer-buffer cell, then clip to the
    // layer's own extent. Layers smaller than the canvas (or offset so that
    // part of the canvas lies outside the buffer) draw as transparent in the
    // out-of-buffer region so lower layers show through.
    let layer_fpos = canvas_fpos - vec2<f32>(layer.offset);
    let layer_wh = vec2<f32>(layer.layer_wh);
    if layer_fpos.x < 0.0 || layer_fpos.y < 0.0
        || layer_fpos.x >= layer_wh.x || layer_fpos.y >= layer_wh.y {
        discard;
    }
    let cell = vec2<u32>(clamp(layer_fpos, vec2<f32>(0.0), layer_wh - vec2<f32>(0.001)));
    let in_cell = fract(layer_fpos);

    let glyph = textureLoad(glyph_tex, vec2<i32>(cell), 0).r;
    let fg = textureLoad(fg_tex, vec2<i32>(cell), 0);
    let bg = textureLoad(bg_tex, vec2<i32>(cell), 0);

    let gx = f32(glyph % 16u);
    let gy = f32(glyph / 16u);
    let atlas_uv = (vec2<f32>(gx, gy) + in_cell) / 16.0;
    let mask = textureSampleLevel(font_tex, font_samp, atlas_uv, 0.0).r;

    let is_transparent = all(bg.rgb == vec3<f32>(1.0, 0.0, 1.0));
    let src_rgb = select(mix(bg.rgb, fg.rgb, mask), fg.rgb, is_transparent);
    let src_a = select(1.0, mask, is_transparent);
    return vec4<f32>(src_rgb, src_a);
}
