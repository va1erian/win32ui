//! The demo's Cube tab: a spinning, depth-tested cube drawn with modern
//! OpenGL, exercising the [`Renderer::Gl`] paint path.
//!
//! The widget returns [`Renderer::Gl`] and builds a 3.3 core-profile program in
//! its first [`paint_gl`](CustomWidget::paint_gl) call, then draws the cube
//! every frame. It asks for animation ticks so the cube spins, and updates its
//! angle on each tick before invalidating.

use std::cell::{Cell, RefCell};

use win32ui::column;
use win32ui::gdi::Canvas;
use win32ui::glow::{self, HasContext};
use win32ui::prelude::*;

use super::Msg;

/// A minimal 4×4 matrix helper (column-major, as OpenGL expects). The demo has
/// no math dependency, so the handful of transforms it needs live here.
mod mat {
    pub type Mat4 = [f32; 16];

    pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
        let mut out = [0.0f32; 16];
        for col in 0..4 {
            for row in 0..4 {
                let mut sum = 0.0;
                for k in 0..4 {
                    sum += a[k * 4 + row] * b[col * 4 + k];
                }
                out[col * 4 + row] = sum;
            }
        }
        out
    }

    pub fn translation(x: f32, y: f32, z: f32) -> Mat4 {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        ]
    }

    pub fn rotation_x(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        [
            1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    pub fn rotation_y(angle: f32) -> Mat4 {
        let (s, c) = angle.sin_cos();
        [
            c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    pub fn perspective(fovy_rad: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
        let f = 1.0 / (fovy_rad / 2.0).tan();
        [
            f / aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            f,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) / (near - far),
            -1.0,
            0.0,
            0.0,
            (2.0 * far * near) / (near - far),
            0.0,
        ]
    }
}

const VERTEX_SRC: &str = r#"
#version 330 core
layout (location = 0) in vec3 a_pos;
layout (location = 1) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_color;
void main() {
    v_color = a_color;
    gl_Position = u_mvp * vec4(a_pos, 1.0);
}
"#;

const FRAGMENT_SRC: &str = r#"
#version 330 core
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}
"#;

/// The cube's eight corners, then one colour per face.
const CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

/// Six quads (counter-clockwise seen from outside) and their face colours.
const FACES: [([usize; 4], [f32; 3]); 6] = [
    ([4, 5, 6, 7], [0.90, 0.22, 0.27]), // front  (+z) red
    ([1, 0, 3, 2], [0.18, 0.80, 0.44]), // back   (-z) green
    ([0, 4, 7, 3], [0.20, 0.60, 0.86]), // left   (-x) blue
    ([5, 1, 2, 6], [0.95, 0.77, 0.06]), // right  (+x) yellow
    ([0, 1, 5, 4], [0.61, 0.35, 0.71]), // bottom (-y) purple
    ([3, 7, 6, 2], [0.10, 0.74, 0.61]), // top    (+y) teal
];

/// The interleaved `position.xyz` + `colour.rgb` vertex buffer for the cube.
fn cube_data() -> Vec<f32> {
    let mut data = Vec::with_capacity(36 * 6);
    for (quad, color) in FACES {
        for index in [0, 1, 2, 0, 2, 3] {
            let corner = CORNERS[quad[index]];
            data.extend_from_slice(&corner);
            data.extend_from_slice(&color);
        }
    }
    data
}

/// The GPU objects built once per GL context. The vertex buffer is bound while
/// the vertex array is set up and is not needed afterwards, so only the program
/// and vertex array are kept; a demo leaks the objects for the process
/// lifetime rather than tracking context teardown.
struct GlResources {
    program: glow::Program,
    vao: glow::VertexArray,
    mvp: Option<glow::UniformLocation>,
}

impl GlResources {
    /// Compiles the shader program and uploads the cube.
    ///
    /// # Safety
    /// A current GL context is required, which the surface holds during a frame.
    unsafe fn build(gl: &glow::Context) -> std::result::Result<GlResources, String> {
        // SAFETY: forwarded to the caller's contract; a context is current.
        unsafe {
            let program = gl.create_program()?;
            let shaders = [
                compile(gl, glow::VERTEX_SHADER, VERTEX_SRC)?,
                compile(gl, glow::FRAGMENT_SHADER, FRAGMENT_SRC)?,
            ];
            for shader in shaders {
                gl.attach_shader(program, shader);
            }
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(gl.get_program_info_log(program));
            }
            for shader in shaders {
                gl.detach_shader(program, shader);
                gl.delete_shader(shader);
            }

            let vao = gl.create_vertex_array()?;
            gl.bind_vertex_array(Some(vao));
            let vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let data = cube_data();
            let bytes = std::slice::from_raw_parts(
                data.as_ptr() as *const u8,
                std::mem::size_of_val(data.as_slice()),
            );
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes, glow::STATIC_DRAW);

            let stride = 6 * std::mem::size_of::<f32>() as i32;
            gl.enable_vertex_attrib_array(0);
            gl.vertex_attrib_pointer_f32(0, 3, glow::FLOAT, false, stride, 0);
            gl.enable_vertex_attrib_array(1);
            gl.vertex_attrib_pointer_f32(1, 3, glow::FLOAT, false, stride, 3 * 4);

            Ok(GlResources {
                program,
                vao,
                mvp: gl.get_uniform_location(program, "u_mvp"),
            })
        }
    }
}

/// Compiles one shader stage, returning its log on failure.
///
/// # Safety
/// A current GL context is required.
unsafe fn compile(
    gl: &glow::Context,
    kind: u32,
    source: &str,
) -> std::result::Result<glow::Shader, String> {
    // SAFETY: forwarded to the caller's contract; a context is current.
    unsafe {
        let shader = gl.create_shader(kind)?;
        gl.shader_source(shader, source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            return Err(gl.get_shader_info_log(shader));
        }
        Ok(shader)
    }
}

/// The spinning cube widget.
struct CubeWidget {
    angle: Cell<f32>,
    resources: RefCell<Option<GlResources>>,
    /// Set once a build failed, so a broken driver is not retried every frame.
    failed: Cell<bool>,
}

impl CubeWidget {
    fn new() -> CubeWidget {
        CubeWidget {
            angle: Cell::new(0.0),
            resources: RefCell::new(None),
            failed: Cell::new(false),
        }
    }
}

impl CustomWidget for CubeWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Gl
    }

    fn paint_gl(&self, gl: &glow::Context, bounds: Rect, _theme: &Theme) {
        let mut resources = self.resources.borrow_mut();
        if resources.is_none() {
            if self.failed.get() {
                return;
            }
            // SAFETY: `paint_gl` is called with the surface's context current.
            match unsafe { GlResources::build(gl) } {
                Ok(built) => *resources = Some(built),
                Err(error) => {
                    self.failed.set(true);
                    eprintln!("demo: cube shaders failed: {error}");
                    return;
                }
            }
        }
        let resources = resources.as_ref().expect("built above");

        let aspect = bounds.width().max(1) as f32 / bounds.height().max(1) as f32;
        let angle = self.angle.get();
        let model = mat::mul(&mat::rotation_y(angle), &mat::rotation_x(angle * 0.6));
        let view = mat::translation(0.0, 0.0, -3.0);
        let projection = mat::perspective(45f32.to_radians(), aspect, 0.1, 100.0);
        let mvp = mat::mul(&projection, &mat::mul(&view, &model));

        // SAFETY: a context is current and every handle belongs to it.
        unsafe {
            gl.use_program(Some(resources.program));
            gl.bind_vertex_array(Some(resources.vao));
            gl.enable(glow::DEPTH_TEST);
            gl.uniform_matrix_4_f32_slice(resources.mvp.as_ref(), false, &mvp);
            gl.draw_arrays(glow::TRIANGLES, 0, 36);
        }
    }

    fn input(&self, input: Input, cx: &mut WidgetCx<()>) {
        match input {
            // Each frame asks for the next one, so the cube keeps spinning.
            Input::Frame => cx.request_animation(true),
            Input::Tick => {
                let angle = self.angle.get() + 0.02;
                self.angle.set(if angle > std::f32::consts::TAU {
                    angle - std::f32::consts::TAU
                } else {
                    angle
                });
                cx.invalidate();
            }
            _ => {}
        }
    }
}

/// The Cube tab, holding the widget so its window stays alive.
pub(super) struct Cube {
    widget: Custom<CubeWidget, Msg>,
}

impl Cube {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Cube {
        let widget = Custom::new(ui, CubeWidget::new()).expect("gl cube widget");
        Cube { widget }
    }

    pub(super) fn page(&self) -> Layout {
        column![self.widget.fill(1)]
    }
}
