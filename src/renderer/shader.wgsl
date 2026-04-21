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

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var font_tex: texture_2d<f32>;
@group(0) @binding(2) var font_samp: sampler;

@group(1) @binding(0) var glyph_tex: texture_2d<u32>;
@group(1) @binding(1) var fg_tex: texture_2d<f32>;
@group(1) @binding(2) var bg_tex: texture_2d<f32>;

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
    let cell_fpos = u.cell_origin + in.uv * u.cell_span;
    let wh = vec2<f32>(u.canvas_wh);
    // Reject fragments that fall outside the canvas bounds — happens when the
    // view is panned so the viewport extends past the canvas edge.
    if cell_fpos.x < 0.0 || cell_fpos.y < 0.0
        || cell_fpos.x >= wh.x || cell_fpos.y >= wh.y {
        discard;
    }
    let cell = vec2<u32>(clamp(cell_fpos, vec2<f32>(0.0), wh - vec2<f32>(0.001)));
    let in_cell = fract(cell_fpos);

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
