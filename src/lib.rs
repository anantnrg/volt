pub mod ui;

use gl::types::*;
use gl_rs as gl;
use glutin::{
    config::{ConfigTemplateBuilder, GlConfig},
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext},
    display::{Display, GetGlDisplay, GlDisplay},
    prelude::{GlSurface, NotCurrentGlContext},
    surface::{Surface as GlutinSurface, SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::DisplayBuilder;
use raw_window_handle::HasRawWindowHandle;
use skia::{
    gpu::{self, backend_render_targets, gl::FramebufferInfo, SurfaceOrigin},
    Color, ColorType, Surface,
};
use std::{ffi::CString, num::NonZeroU32};
use winit::{
    dpi::LogicalSize,
    event::{Event, KeyEvent, Modifiers, WindowEvent},
    event_loop::EventLoop,
    window::{Window, WindowBuilder},
};

// Re-exports
pub use skia::font_style;

use crate::ui::Component;

pub struct Volt {
    modifiers: Modifiers,
    paint: skia::Paint,
    surface: Surface,
    gl_surface: GlutinSurface<WindowSurface>,
    gr_context: skia::gpu::DirectContext,
    gl_context: PossiblyCurrentContext,
    window: Window,
    event_loop: Option<EventLoop<()>>,
}

impl Volt {
    pub fn new(
        title: &str,
        win_width: u32,
        win_height: u32,
        fill: skia::Color,
    ) -> anyhow::Result<Self> {
        let event_loop = EventLoop::new()?;
        let winit_window_builder = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(LogicalSize::new(win_width, win_height));
        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_transparency(true);
        let display_builder = DisplayBuilder::new().with_window_builder(Some(winit_window_builder));

        let (window, gl_config) = display_builder
            .build(&event_loop, template, |configs| {
                configs
                    .reduce(|accum, config| {
                        let transparency_check = config.supports_transparency().unwrap_or(false)
                            & !accum.supports_transparency().unwrap_or(false);

                        if transparency_check || config.num_samples() < accum.num_samples() {
                            config
                        } else {
                            accum
                        }
                    })
                    .unwrap()
            })
            .unwrap();

        let window = window.expect("Couldn't create a window.");
        let raw_window_handle = window.raw_window_handle();

        let context_attributes = ContextAttributesBuilder::new().build(Some(raw_window_handle));
        let fallback_context_attributes = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(None))
            .build(Some(raw_window_handle));

        let not_current_gl_context = unsafe {
            gl_config
                .display()
                .create_context(&gl_config, &context_attributes)
                .unwrap_or_else(|_| {
                    gl_config
                        .display()
                        .create_context(&gl_config, &fallback_context_attributes)
                        .expect("failed to create context")
                })
        };

        let (width, height): (u32, u32) = window.inner_size().into();

        let attrs = SurfaceAttributesBuilder::<WindowSurface>::new().build(
            raw_window_handle,
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let gl_surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &attrs)
                .expect("Could not create gl window surface")
        };

        let gl_context = not_current_gl_context
            .make_current(&gl_surface)
            .expect("Could not make GL context current when setting up skia renderer");

        gl::load_with(|s| {
            gl_config
                .display()
                .get_proc_address(CString::new(s).unwrap().as_c_str())
        });
        let interface = skia::gpu::gl::Interface::new_load_with(|name| {
            if name == "eglGetCurrentDisplay" {
                return std::ptr::null();
            }
            gl_config
                .display()
                .get_proc_address(CString::new(name).unwrap().as_c_str())
        })
        .expect("Could not create interface");

        let mut gr_context = skia::gpu::DirectContext::new_gl(Some(interface), None)
            .expect("Could not create direct context");

        let fb_info = {
            let mut fboid: GLint = 0;
            unsafe { gl::GetIntegerv(gl::FRAMEBUFFER_BINDING, &mut fboid) };

            FramebufferInfo {
                fboid: fboid.try_into().unwrap(),
                format: skia::gpu::gl::Format::RGBA8.into(),
                ..Default::default()
            }
        };
        let num_samples = gl_config.num_samples() as usize;
        let stencil_size = gl_config.stencil_size() as usize;

        let surface =
            Self::create_surface(&window, fb_info, &mut gr_context, num_samples, stencil_size);

        let mut modifiers = Modifiers::default();
        let mut paint = skia::Paint::default();

        Ok(Volt {
            surface,
            gl_surface,
            gl_context,
            gr_context,
            window,
            modifiers,
            paint,
            event_loop: Some(event_loop),
        })
    }
    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = self.event_loop.take().unwrap();
        event_loop.run(move |event, window_target| {
            let mut draw_frame = false;

            if let Event::WindowEvent { event, .. } = event {
                match event {
                    WindowEvent::CloseRequested => {
                        window_target.exit();
                        return;
                    }
                    WindowEvent::ModifiersChanged(new_modifiers) => self.modifiers = new_modifiers,
                    WindowEvent::KeyboardInput {
                        event: KeyEvent { logical_key, .. },
                        ..
                    } => {
                        if self.modifiers.state().super_key() && logical_key == "q" {
                            window_target.exit();
                        }
                        self.window.request_redraw();
                    }
                    WindowEvent::RedrawRequested => {
                        draw_frame = true;
                    }
                    _ => (),
                }
            }
            if draw_frame {
                let canvas = self.surface.canvas();
                canvas.clear(Color::from_rgb(30, 29, 45));
                let mut button = crate::ui::button::Button {
                    text: "Something",
                    position: (10.0, 10.0),
                    size: (200.0, 50.0),
                    color: crate::ui::Color::Hex("#cdd6f4".to_string()).into().unwrap(),
                    radius: 10.0,
                    border_width: 0.0,
                    border_color: skia::Color::RED,
                    text_color: skia::Color::RED,
                    font_size: 27.0,
                    font_family: "JetBrains Mono",
                    font_weight: font_style::Weight::BOLD,
                    font_style: font_style::Slant::Italic,
                };
                button.set_text("hello");
                button.render(canvas, &mut self.paint);
                self.gr_context.flush_and_submit();
                self.gl_surface.swap_buffers(&self.gl_context).unwrap();
            }
        })?;

        Ok(())
    }
    pub fn create_surface(
        window: &Window,
        fb_info: FramebufferInfo,
        gr_context: &mut skia::gpu::DirectContext,
        num_samples: usize,
        stencil_size: usize,
    ) -> Surface {
        let size = window.inner_size();
        let size = (
            size.width.try_into().expect("Could not convert width"),
            size.height.try_into().expect("Could not convert height"),
        );
        let backend_render_target =
            backend_render_targets::make_gl(size, num_samples, stencil_size, fb_info);

        gpu::surfaces::wrap_backend_render_target(
            gr_context,
            &backend_render_target,
            SurfaceOrigin::BottomLeft,
            ColorType::RGBA8888,
            None,
            None,
        )
        .expect("Could not create skia surface")
    }
}
