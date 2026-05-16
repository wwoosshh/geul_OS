//! GeulOS 컴포지터: 객체 트리를 host OS 윈도우에 그린다.

use std::num::NonZeroU32;
use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    surface: Option<softbuffer::Surface<Arc<Window>, Arc<Window>>>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("GeulOS Compositor (M4)")
            .with_inner_size(PhysicalSize::new(800u32, 600u32));
        let window = Arc::new(event_loop.create_window(attrs).expect("create_window"));
        let context = softbuffer::Context::new(window.clone()).expect("softbuffer Context");
        let surface = softbuffer::Surface::new(&context, window.clone()).expect("softbuffer Surface");
        self.window = Some(window);
        self.surface = Some(surface);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(window), Some(surface)) = (&self.window, &mut self.surface) {
                    let size = window.inner_size();
                    let (w, h) = (size.width, size.height);
                    if w == 0 || h == 0 {
                        return;
                    }
                    surface
                        .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
                        .expect("resize");
                    let mut buffer = surface.buffer_mut().expect("buffer_mut");
                    // 흰 배경
                    for px in buffer.iter_mut() {
                        *px = 0xFF_FF_FF_FF; // 0xAARRGGBB on softbuffer (top byte ignored)
                    }
                    buffer.present().expect("present");
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("EventLoop");
    let mut app = App { window: None, surface: None };
    event_loop.run_app(&mut app).expect("run_app");
}
