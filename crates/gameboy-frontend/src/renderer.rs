use eframe::glow::{self, HasContext};
use egui::PaintCallbackInfo;

use gameboy_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

const NATIVE_W: i32 = SCREEN_WIDTH as i32;
const NATIVE_H: i32 = SCREEN_HEIGHT as i32;

#[derive(Clone, Copy)]
pub struct ShaderParams {
    pub color_correct: bool,
    pub gamma_weight: f32,
    pub ghosting: bool,
    pub response_time: f32,
    pub pixel_aa: bool,
    pub lcd_grid: bool,
    pub grid_intensity: f32,
}

const VERT: &str = r#"
out vec2 v_uv;
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    v_uv = p;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
"#;

const FRAG_COLOR: &str = r#"
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex;
uniform float u_gamma;

const mat3 GBC = mat3(
    26.0 / 32.0, 0.0,          6.0 / 32.0,
     4.0 / 32.0, 24.0 / 32.0,  4.0 / 32.0,
     2.0 / 32.0,  8.0 / 32.0, 22.0 / 32.0
);

void main() {
    vec3 c = texture(u_tex, v_uv).rgb;
    float exponent = mix(1.0, 1.5, clamp(u_gamma, 0.0, 1.0));
    c = pow(c, vec3(exponent));
    c = clamp(GBC * c, 0.0, 1.0);
    frag = vec4(c, 1.0);
}
"#;

const FRAG_TEMPORAL: &str = r#"
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex;
uniform sampler2D u_prev;
uniform float u_response;

void main() {
    vec3 cur = texture(u_tex, v_uv).rgb;
    vec3 prev = texture(u_prev, v_uv).rgb;
    float k = clamp(u_response, 0.0, 0.95);
    frag = vec4(mix(cur, prev, k), 1.0);
}
"#;

const FRAG_PIXEL_AA: &str = r#"
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex;
uniform vec2 u_native;
uniform vec2 u_output;
uniform int u_aa;
uniform int u_flip;

void main() {
    vec2 src_uv = v_uv;
    if (u_flip == 1) src_uv.y = 1.0 - src_uv.y;
    vec2 tsize = u_native;
    vec2 texel = src_uv * tsize;
    if (u_aa == 0) {
        vec2 nearest = (floor(texel) + 0.5) / tsize;
        frag = vec4(texture(u_tex, nearest).rgb, 1.0);
        return;
    }
    vec2 scale = u_output / tsize;
    vec2 tfloor = floor(texel);
    vec2 s = fract(texel) - 0.5;
    vec2 region = 0.5 - 0.5 / max(scale, vec2(1.0));
    vec2 f = (s - clamp(s, -region, region)) * scale + 0.5;
    vec2 uv = (tfloor + f) / tsize;
    frag = vec4(texture(u_tex, uv).rgb, 1.0);
}
"#;

const FRAG_GRID: &str = r#"
in vec2 v_uv;
out vec4 frag;
uniform sampler2D u_tex;
uniform float u_intensity;
uniform float u_scale;
uniform int u_flip;

void main() {
    vec2 uv = v_uv;
    if (u_flip == 1) uv.y = 1.0 - uv.y;
    vec3 c = texture(u_tex, uv).rgb;

    float col_w = max(u_scale / 3.0, 1.0);
    float idx = mod(floor(gl_FragCoord.x / col_w), 3.0);
    vec3 mask = idx < 0.5 ? vec3(1.0, 0.55, 0.55)
              : idx < 1.5 ? vec3(0.55, 1.0, 0.55)
                          : vec3(0.55, 0.55, 1.0);

    float cell = max(u_scale, 1.0);
    float y = gl_FragCoord.y / cell;
    float fy = fract(y);
    float lw = fwidth(y) * 1.5;
    float gap = smoothstep(0.0, lw, fy) * smoothstep(0.0, lw, 1.0 - fy);
    gap = mix(0.55, 1.0, gap);

    vec3 tinted = c * mask * gap;
    frag = vec4(mix(c, tinted, clamp(u_intensity, 0.0, 1.0)), 1.0);
}
"#;

struct Pass {
    program: glow::Program,
}

impl Pass {
    fn new(gl: &glow::Context, header: &str, frag: &str) -> Result<Self, String> {
        unsafe {
            let program = gl.create_program()?;
            let shaders = [(glow::VERTEX_SHADER, VERT), (glow::FRAGMENT_SHADER, frag)];
            let mut compiled = Vec::new();
            for (kind, src) in shaders {
                let shader = gl.create_shader(kind)?;
                gl.shader_source(shader, &format!("{header}{src}"));
                gl.compile_shader(shader);
                if !gl.get_shader_compile_status(shader) {
                    let log = gl.get_shader_info_log(shader);
                    return Err(format!("shader compile failed: {log}"));
                }
                gl.attach_shader(program, shader);
                compiled.push(shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!(
                    "program link failed: {}",
                    gl.get_program_info_log(program)
                ));
            }
            for shader in compiled {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }
            Ok(Self { program })
        }
    }

    fn bind(&self, gl: &glow::Context) {
        unsafe { gl.use_program(Some(self.program)) };
    }

    fn set_i32(&self, gl: &glow::Context, name: &str, v: i32) {
        unsafe {
            let loc = gl.get_uniform_location(self.program, name);
            gl.uniform_1_i32(loc.as_ref(), v);
        }
    }

    fn set_f32(&self, gl: &glow::Context, name: &str, v: f32) {
        unsafe {
            let loc = gl.get_uniform_location(self.program, name);
            gl.uniform_1_f32(loc.as_ref(), v);
        }
    }

    fn set_vec2(&self, gl: &glow::Context, name: &str, x: f32, y: f32) {
        unsafe {
            let loc = gl.get_uniform_location(self.program, name);
            gl.uniform_2_f32(loc.as_ref(), x, y);
        }
    }
}

pub struct Pipeline {
    color: Pass,
    temporal: Pass,
    pixel_aa: Pass,
    grid: Pass,

    vao: glow::VertexArray,
    fbo: glow::Framebuffer,

    src: glow::Texture,
    scratch_a: glow::Texture,
    history: [glow::Texture; 2],
    hist_read: usize,

    out_tex: glow::Texture,
    out_size: (i32, i32),

    primed: bool,
}

impl Pipeline {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let header = shader_header(gl);
        unsafe {
            let vao = gl.create_vertex_array()?;
            let fbo = gl.create_framebuffer()?;

            let src = new_texture(gl, NATIVE_W, NATIVE_H);
            let scratch_a = new_texture(gl, NATIVE_W, NATIVE_H);
            let history = [
                new_texture(gl, NATIVE_W, NATIVE_H),
                new_texture(gl, NATIVE_W, NATIVE_H),
            ];
            let out_tex = new_texture(gl, NATIVE_W, NATIVE_H);

            Ok(Self {
                color: Pass::new(gl, &header, FRAG_COLOR)?,
                temporal: Pass::new(gl, &header, FRAG_TEMPORAL)?,
                pixel_aa: Pass::new(gl, &header, FRAG_PIXEL_AA)?,
                grid: Pass::new(gl, &header, FRAG_GRID)?,
                vao,
                fbo,
                src,
                scratch_a,
                history,
                hist_read: 0,
                out_tex,
                out_size: (NATIVE_W, NATIVE_H),
                primed: false,
            })
        }
    }

    pub fn render(
        &mut self,
        gl: &glow::Context,
        rgba: &[u8],
        p: &ShaderParams,
        info: &PaintCallbackInfo,
    ) {
        let vp = info.viewport_in_pixels();
        let (out_w, out_h) = (vp.width_px.max(1), vp.height_px.max(1));

        unsafe {
            let prev_fbo = current_framebuffer(gl);

            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.bind_vertex_array(Some(self.vao));
            gl.active_texture(glow::TEXTURE0);

            if !self.primed {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                for tex in self.history {
                    self.attach(gl, tex);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }
                self.primed = true;
            }

            gl.bind_texture(glow::TEXTURE_2D, Some(self.src));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                NATIVE_W,
                NATIVE_H,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(rgba)),
            );

            gl.viewport(0, 0, NATIVE_W, NATIVE_H);
            let mut cur = self.src;

            if p.color_correct {
                self.attach(gl, self.scratch_a);
                self.color.bind(gl);
                bind_tex(gl, 0, cur);
                self.color.set_i32(gl, "u_tex", 0);
                self.color.set_f32(gl, "u_gamma", p.gamma_weight);
                draw(gl);
                cur = self.scratch_a;
            }

            if p.ghosting {
                let read = self.history[self.hist_read];
                let write = self.history[1 - self.hist_read];
                self.attach(gl, write);
                self.temporal.bind(gl);
                bind_tex(gl, 0, cur);
                bind_tex(gl, 1, read);
                self.temporal.set_i32(gl, "u_tex", 0);
                self.temporal.set_i32(gl, "u_prev", 1);
                self.temporal.set_f32(gl, "u_response", p.response_time);
                draw(gl);
                gl.active_texture(glow::TEXTURE0);
                self.hist_read = 1 - self.hist_read;
                cur = write;
            }

            if p.lcd_grid {
                self.ensure_out_tex(gl, out_w, out_h);
                gl.viewport(0, 0, out_w, out_h);
                self.attach(gl, self.out_tex);
                self.run_pixel_aa(gl, cur, out_w as f32, out_h as f32, p.pixel_aa, false);

                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fbo);
                gl.viewport(vp.left_px, vp.from_bottom_px, out_w, out_h);
                self.grid.bind(gl);
                bind_tex(gl, 0, self.out_tex);
                self.grid.set_i32(gl, "u_tex", 0);
                self.grid.set_f32(gl, "u_intensity", p.grid_intensity);
                self.grid
                    .set_f32(gl, "u_scale", out_h as f32 / NATIVE_H as f32);
                self.grid.set_i32(gl, "u_flip", 1);
                draw(gl);
            } else {
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fbo);
                gl.viewport(vp.left_px, vp.from_bottom_px, out_w, out_h);
                self.run_pixel_aa(gl, cur, out_w as f32, out_h as f32, p.pixel_aa, true);
            }

            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, prev_fbo);
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }

    unsafe fn attach(&self, gl: &glow::Context, tex: glow::Texture) {
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(self.fbo));
        gl.framebuffer_texture_2d(
            glow::DRAW_FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(tex),
            0,
        );
    }

    unsafe fn run_pixel_aa(
        &self,
        gl: &glow::Context,
        src: glow::Texture,
        out_w: f32,
        out_h: f32,
        aa: bool,
        flip: bool,
    ) {
        self.pixel_aa.bind(gl);
        bind_tex(gl, 0, src);
        self.pixel_aa.set_i32(gl, "u_tex", 0);
        self.pixel_aa
            .set_vec2(gl, "u_native", NATIVE_W as f32, NATIVE_H as f32);
        self.pixel_aa.set_vec2(gl, "u_output", out_w, out_h);
        self.pixel_aa.set_i32(gl, "u_aa", aa as i32);
        self.pixel_aa.set_i32(gl, "u_flip", flip as i32);
        draw(gl);
    }

    unsafe fn ensure_out_tex(&mut self, gl: &glow::Context, w: i32, h: i32) {
        if self.out_size != (w, h) {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.out_tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.out_size = (w, h);
        }
    }
}

fn shader_header(gl: &glow::Context) -> String {
    let version = unsafe { gl.get_parameter_string(glow::SHADING_LANGUAGE_VERSION) };
    if version.contains("ES") || version.contains("WebGL") {
        "#version 300 es\nprecision highp float;\n".to_owned()
    } else {
        "#version 140\n".to_owned()
    }
}

unsafe fn new_texture(gl: &glow::Context, w: i32, h: i32) -> glow::Texture {
    let tex = gl.create_texture().expect("create texture");
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        w,
        h,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(None),
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::LINEAR as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_S,
        glow::CLAMP_TO_EDGE as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_WRAP_T,
        glow::CLAMP_TO_EDGE as i32,
    );
    tex
}

unsafe fn bind_tex(gl: &glow::Context, unit: u32, tex: glow::Texture) {
    gl.active_texture(glow::TEXTURE0 + unit);
    gl.bind_texture(glow::TEXTURE_2D, Some(tex));
}

unsafe fn draw(gl: &glow::Context) {
    gl.draw_arrays(glow::TRIANGLES, 0, 3);
}

unsafe fn current_framebuffer(gl: &glow::Context) -> Option<glow::Framebuffer> {
    let id = gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING);
    std::num::NonZeroU32::new(id as u32).map(glow::NativeFramebuffer)
}
