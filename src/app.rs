use crate::graphics::{create_graphics, Graphics, Rc, Vertex};
use spade::{DelaunayTriangulation, Point2, Triangulation};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

enum State {
    Ready(Graphics),
    Init(Option<EventLoopProxy<Graphics>>),
}

#[derive(Debug)]
struct PhysicalVertex {
    pos: Vec2,
    val: f32,
}

impl PhysicalVertex {
    fn to_display_vertex(&self, aspect_ratio: f32) -> Vertex {
        let PhysicalVertex { pos, val } = self;
        Vertex {
            position: [pos.x * aspect_ratio, pos.y],
            color: *val,
            tex_coords: [pos.x * aspect_ratio, pos.y],
        }
    }
}

#[derive(Debug, Default)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    fn abs(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }
}

impl std::ops::Add for &Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Self) -> Self::Output {
        Self::Output {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for &Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::Output {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::Div<f32> for &Vec2 {
    type Output = Vec2;

    fn div(self, rhs: f32) -> Self::Output {
        Self::Output {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}
impl std::ops::Mul<f32> for &Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::Output {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

#[derive(Debug)]
struct Mass {
    mass: f32,
    pos: Vec2,
}

impl Default for Mass {
    fn default() -> Self {
        Self {
            mass: 1.0,
            pos: Vec2 { x: 0.3, y: 0.4 },
        }
    }
}

struct Force {
    v: Vec2,
}

impl Mass {
    pub fn force(&self, other: &PhysicalVertex) -> Force {
        let PhysicalVertex { pos: other_pos, .. } = other;
        let Mass {
            mass,
            pos: mass_pos,
        } = self;
        let mut v = mass_pos - other_pos;
        let magnitude = 1e-3 * mass / v.abs().powf(2.0);
        v = &v * (magnitude / (v.abs() as f32));
        Force { v }
    }
}

struct Physics {
    vertices: Vec<PhysicalVertex>,
    indices: Vec<u32>,
    mass: Mass,
}

impl Physics {
    /// create a random distribution of points in the [-1,1]^2 square, with
    /// approximately n_sqrt*n_sqrt vertices in total.
    fn rand(n_sqrt: usize) -> Self {
        let mut points: Vec<Point2<f32>> = (0..n_sqrt.pow(2))
            .map(|_| {
                Point2::new(
                    rand::random::<f32>() * 2.0 - 1.0,
                    rand::random::<f32>() * 2.0 - 1.0,
                )
            })
            .collect();
        points.push(Point2 { x: -1.0, y: -1.0 });
        points.push(Point2 { x: -1.0, y: 1.0 });
        points.push(Point2 { x: 1.0, y: -1.0 });
        points.push(Point2 { x: 1.0, y: 1.0 });
        for i in 0..n_sqrt {
            let p = -1.0 + i as f32 * 2.0 / n_sqrt as f32;
            points.push(Point2 { x: -1.0, y: p });
            points.push(Point2 { x: 1.0, y: p });
            points.push(Point2 { x: p, y: -1.0 });
            points.push(Point2 { x: p, y: 1.0 });
        }
        // TODO: add points on border to guarantee complete hull
        let mut triangulation: DelaunayTriangulation<_> = DelaunayTriangulation::new();
        for point in points {
            triangulation.insert(point).unwrap();
        }
        let mut vertices = vec![];
        let mut indices = vec![];
        for vertex in triangulation.vertices() {
            vertices.push(PhysicalVertex {
                pos: Vec2 {
                    x: vertex.position().x,
                    y: vertex.position().y,
                },
                val: rand::random(),
            });
        }
        for face in triangulation.inner_faces() {
            for vertex in face.vertices() {
                indices.push(vertex.index() as u32);
            }
        }
        Self {
            vertices,
            indices,
            mass: Mass::default(),
        }
    }
    pub fn update(&mut self) {
        let Physics { vertices, mass, .. } = self;
        for vertex in vertices {
            vertex.val = 0.0;
            vertex.val = mass.force(vertex).v.abs();
        }
    }

    fn display_vertices(&self, aspect_ratio: f32) -> Vec<Vertex> {
        self.vertices
            .iter()
            .map(|v| v.to_display_vertex(aspect_ratio))
            .collect()
    }
}

pub struct App {
    state: State,
    physics: Physics,
}

impl App {
    pub fn new(event_loop: &EventLoop<Graphics>) -> Self {
        Self {
            state: State::Init(Some(event_loop.create_proxy())),
            physics: Physics::rand(100),
            // physics: Physics::triangle(),
        }
    }

    fn draw(&mut self) {
        if let State::Ready(gfx) = &mut self.state {
            let size = gfx.size();
            let aspect_ratio = size.1 as f32 / size.0 as f32;
            println!("{:?}", self.physics.vertices);
            self.physics.update();
            gfx.push_vertices(
                self.physics.display_vertices(aspect_ratio),
                self.physics.indices.as_slice(),
            );
            gfx.draw();
        }
    }

    fn resized(&mut self, size: PhysicalSize<u32>) {
        if let State::Ready(gfx) = &mut self.state {
            gfx.resize(size);
        }
    }
}

impl ApplicationHandler<Graphics> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let State::Init(proxy) = &mut self.state {
            if let Some(proxy) = proxy.take() {
                let mut win_attr = Window::default_attributes();

                #[cfg(not(target_arch = "wasm32"))]
                {
                    win_attr = win_attr.with_title("WebGPU example");
                }

                #[cfg(target_arch = "wasm32")]
                {
                    use winit::platform::web::WindowAttributesExtWebSys;
                    win_attr = win_attr.with_append(true);
                }

                let window = Rc::new(
                    event_loop
                        .create_window(win_attr)
                        .expect("create window err."),
                );

                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_futures::spawn_local(create_graphics(window, proxy));

                #[cfg(not(target_arch = "wasm32"))]
                pollster::block_on(create_graphics(window, proxy));
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, graphics: Graphics) {
        // Request a redraw now that graphics are ready
        graphics.request_redraw();
        self.state = State::Ready(graphics);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => self.resized(size),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}
