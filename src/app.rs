use std::iter::Sum;

use crate::graphics::{create_graphics, Graphics, Rc, Vertex};
use rand::{
    distr::{Distribution, StandardUniform},
    RngExt,
};
use spade::{DelaunayTriangulation, Point2, Triangulation};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::{Window, WindowId},
};

const REPETITIONS: isize = 10;

enum State {
    Ready(Graphics),
    Init(Option<EventLoopProxy<Graphics>>),
}

#[derive(Debug)]
struct PhysicalVertex {
    pos: Vec2,
    acc: Vec2,
}

impl PhysicalVertex {
    fn to_display_vertex(&self, aspect_ratio: f32) -> Vertex {
        let PhysicalVertex { pos, acc } = self;
        Vertex {
            position: [pos.x * aspect_ratio, pos.y],
            color: acc.abs() * 1e-5,
            tex_coords: [pos.x * aspect_ratio, pos.y],
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct Vec2 {
    x: f32,
    y: f32,
}

impl Vec2 {
    fn abs(&self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    /// only use for position-like Vec2 objects. Wraps positions into the
    /// [-1,1]^2 window
    fn wrap(&mut self) {
        self.x = self.x % 2.0;
        self.y = self.y % 2.0;
    }
}

impl std::ops::Add for Vec2 {
    type Output = Vec2;

    fn add(self, rhs: Self) -> Self::Output {
        Self::Output {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Vec2 {
    type Output = Vec2;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::Output {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl std::ops::Div<f32> for Vec2 {
    type Output = Vec2;

    fn div(self, rhs: f32) -> Self::Output {
        Self::Output {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}
impl std::ops::Mul<f32> for Vec2 {
    type Output = Vec2;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::Output {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Sum for Vec2 {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.reduce(|a, b| a + b).unwrap()
    }
}

impl rand::distr::Distribution<Vec2> for StandardUniform {
    fn sample<R: rand::prelude::Rng + ?Sized>(&self, rng: &mut R) -> Vec2 {
        Vec2 {
            x: -1.0 + 2.0 * rng.random::<f32>(),
            y: -1.0 + 2.0 * rng.random::<f32>(),
        }
    }
}

#[derive(Debug, Clone)]
struct Mass {
    mass: f32,
    pos: Vec2,
    vel: Vec2,
}

impl Mass {
    pub fn acc(&self, other_pos: &Vec2) -> Vec2 {
        let Mass {
            mass,
            pos: mass_pos,
            ..
        } = self;

        let mut total = Vec2 { x: 0.0, y: 0.0 };
        for x_wraps in -REPETITIONS..(REPETITIONS + 1) {
            let mut offset = Vec2 {
                x: 2.0 * x_wraps as f32,
                y: 0.0,
            };
            for y_wraps in -REPETITIONS..(REPETITIONS + 1) {
                offset.y = 2.0 * y_wraps as f32;
                let local = *mass_pos + offset - *other_pos;
                let magnitude = 1e-4 * mass / (local.abs() + 1e-2).powf(2.0);
                total = total + local * (magnitude / (local.abs() as f32));
            }
        }
        total
    }
}

impl Distribution<Mass> for StandardUniform {
    fn sample<R: rand::prelude::Rng + ?Sized>(&self, rng: &mut R) -> Mass {
        Mass {
            mass: 10f32.powf(4.0 * rng.random::<f32>()),
            pos: rng.random(),
            vel: rng.random::<Vec2>() * 10.0,
        }
    }
}

struct Physics {
    vertices: Vec<PhysicalVertex>,
    indices: Vec<u32>,
    masses: Vec<Mass>,
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
                acc: Vec2 { x: 0.0, y: 0.0 },
            });
        }
        for face in triangulation.inner_faces() {
            for vertex in face.vertices() {
                indices.push(vertex.index() as u32);
            }
        }
        let masses = vec![rand::random(), rand::random()];
        Self {
            vertices,
            indices,
            masses,
        }
    }
    pub fn update(&mut self) {
        const DT: f32 = 1e-3;
        let Physics {
            vertices, masses, ..
        } = self;

        // update gravity field
        for vertex in vertices {
            vertex.acc = masses.iter().map(|m| m.acc(&vertex.pos)).sum();
        }

        // propagate particles
        let old_masses = masses.clone();
        for (i, mass) in masses.iter_mut().enumerate() {
            let acc: Vec2 = old_masses
                .iter()
                .enumerate()
                .filter(|(j, _)| i != *j)
                .map(|(_, m)| m.acc(&mass.pos))
                .sum();
            mass.vel = mass.vel + acc * DT ;
            mass.vel = mass.vel * (1.0 - (mass.vel.abs() / 1e6).powf(2.0));
            mass.pos = mass.pos + mass.vel * DT;
            mass.pos.wrap();
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
        }
    }

    fn draw(&mut self) {
        if let State::Ready(gfx) = &mut self.state {
            let size = gfx.size();
            let aspect_ratio = size.1 as f32 / size.0 as f32;
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
            WindowEvent::RedrawRequested => {
                self.draw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    // Emitted once all pending events have been processed, just before the loop waits
    // again. ControlFlow::Poll keeps the loop spinning, but a frame is only drawn when
    // something explicitly asks for one, so requesting the next redraw here is what
    // turns that spinning into a continuous render loop. Without this, redraws only
    // happen when the compositor asks for them (resize, focus, etc.).
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let State::Ready(gfx) = &self.state {
            gfx.request_redraw();
        }
    }
}