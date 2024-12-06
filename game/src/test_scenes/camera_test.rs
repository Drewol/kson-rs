use std::fmt::Display;
use std::path::PathBuf;

use di::ServiceProvider;
use glam::{vec2, Mat4, Vec3, Vec4};

use palette::WithAlpha;
use wgpu::SurfaceTexture;

use crate::game::camera::ChartCamera;
use crate::game::chart_view::ChartView;
use crate::help::RenderContext;
use crate::scene::Scene;
use crate::shaded_mesh::ShadedMesh;
use crate::Viewport;

#[derive(PartialEq, PartialOrd)]
enum TestBg {
    None,
    Ksm,
    Sdvx,
}

impl Display for TestBg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestBg::None => "None",
            TestBg::Ksm => "KSM",
            TestBg::Sdvx => "SDVX",
        }
        .fmt(f)
    }
}

pub struct CameraTest {
    camera: ChartCamera,
    close: bool,
    track_shader: ShadedMesh,
    bg: TestBg,
}

impl CameraTest {
    pub fn new(service_provider: ServiceProvider, skin_folder: PathBuf) -> Self {
        let context = service_provider
            .get_required::<RenderContext>()
            .as_ref()
            .clone();

        let mut shader_folder = skin_folder.clone();
        let mut texture_folder = skin_folder.clone();
        shader_folder.push("shaders");
        texture_folder.push("textures");
        texture_folder.push("dummy.png");

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(&crate::game::graphics::xy_rect(
            Vec3::ZERO,
            vec2(1.0, ChartView::TRACK_LENGTH * 2.0),
        ));

        track_shader.set_param(
            "lCol",
            Vec4::from(
                palette::named::BLUE
                    .into_format()
                    .with_alpha(1.0)
                    .into_components(),
            ),
        );
        track_shader.set_param(
            "rCol",
            Vec4::from(
                palette::named::RED
                    .into_format()
                    .with_alpha(1.0)
                    .into_components(),
            ),
        );

        track_shader
            .use_texture(
                "mainTex",
                texture_folder.with_file_name("track.png"),
                (false, false),
                true,
            )
            .expect("Failed to set texture uniform");
        Self {
            camera: ChartCamera::new(),
            close: false,
            track_shader,
            bg: TestBg::None,
        }
    }
}

impl Scene for CameraTest {
    fn render_ui(&mut self, _dt: f64) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_suspended(&self) -> bool {
        false
    }

    fn debug_ui(&mut self, _ctx: &egui::Context) -> anyhow::Result<()> {
        Ok(())
    }

    fn closed(&self) -> bool {
        self.close
    }

    fn name(&self) -> &str {
        "Camera Test"
    }

    fn has_egui(&self) -> bool {
        true
    }

    fn render_egui(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        egui::Window::new("Camera Test").show(ctx, |ui| {
            self.camera.egui_widget(ui);
            ui.end_row();

            egui::ComboBox::from_label("BG")
                .selected_text(self.bg.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.bg, TestBg::None, "None");
                    ui.selectable_value(&mut self.bg, TestBg::Ksm, "KSM");
                    ui.selectable_value(&mut self.bg, TestBg::Sdvx, "SDVX");
                });

            if ui.button("Close").clicked() {
                self.close = true;
            }
        });
        Ok(())
    }

    fn render(
        &mut self,
        _dt: f64,
        _td_context: &RenderContext,
        viewport: Viewport,
        _surface: &SurfaceTexture,
    ) {
        self.camera
            .update(vec2(viewport.width as f32, viewport.height as f32));
        let td_camera: Mat4 = Mat4::from(&self.camera);
    }
}
