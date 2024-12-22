use std::fmt::Display;
use std::path::PathBuf;
use std::rc::Rc;

use di::ServiceProvider;
use glam::{vec2, Mat4, Vec3, Vec4};

use itertools::Itertools;
use palette::WithAlpha;
use wgpu::SurfaceTexture;

use crate::game::camera::ChartCamera;
use crate::game::chart_view::ChartView;
use crate::game::graphics::extend_mesh;
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

        let track_mesh =
            crate::game::graphics::xy_rect(Vec3::ZERO, vec2(1.0, ChartView::TRACK_LENGTH * 2.0));
        let mut crit_mesh = crate::game::graphics::xy_rect(Vec3::ZERO, vec2(2.0, 0.02));

        crit_mesh
            .uvs
            .as_mut()
            .unwrap()
            .iter_mut()
            .for_each(|x| *x = vec2(1.0, 1.0));

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(&extend_mesh(track_mesh, crit_mesh));

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
                "main_tex",
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
    fn render_ui(
        &mut self,
        _dt: f64,
    ) -> std::result::Result<std::vec::Vec<wgpu::CommandBuffer>, anyhow::Error> {
        Ok(vec![])
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
        surface: Rc<SurfaceTexture>,
    ) -> std::vec::Vec<wgpu::CommandBuffer> {
        self.camera
            .update(vec2(viewport.width as f32, viewport.height as f32));
        let camera: three_d_asset::Camera = (&self.camera).into();
        self.track_shader
            .draw_camera(&camera, surface)
            .into_iter()
            .collect_vec()
    }
}
