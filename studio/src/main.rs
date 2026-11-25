//! Alexander: open photos, give them the look of the book's pictures live, export.
//!
//!     alexander-studio [photos or folders…]

mod app;

use std::sync::Arc;

fn main() -> eframe::Result {
    let paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    #[cfg(target_os = "macos")]
    app::finder::listen();
    let mut wgpu_options = egui_wgpu::WgpuConfiguration::default();
    if let egui_wgpu::WgpuSetup::CreateNew(create) = &mut wgpu_options.wgpu_setup {
        create.device_descriptor = Arc::new(|adapter: &wgpu::Adapter| {
            let base = if adapter.get_info().backend == wgpu::Backend::Gl { wgpu::Limits::downlevel_webgl2_defaults() } else { wgpu::Limits::default() };
            wgpu::DeviceDescriptor {
                label: Some("studio"),
                required_features: alexander_studio::gpu::wanted_features(adapter),
                // large photos at their own size want textures past eframe's 8192
                required_limits: wgpu::Limits { max_texture_dimension_2d: adapter.limits().max_texture_dimension_2d.min(16384), ..base },
                memory_hints: wgpu::MemoryHints::Performance,
            }
        });
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options,
        viewport: egui::ViewportBuilder::default().with_title("Alexander").with_icon(egui::IconData::default()).with_inner_size([1440.0, 900.0]).with_min_inner_size([720.0, 480.0]).with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Alexander", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, paths)))))
}
