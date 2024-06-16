use std::path::PathBuf;
use std::sync::Arc;

use di::ServiceProvider;
use three_d::{
    vec2, Camera, ColorMaterial, CpuMaterial, CpuTexture, Deg, Gm, Mat3, Matrix4, Mesh, Rad,
    Rectangle, Srgba, Texture2DRef, Vec2, Vec3, Zero,
};

use crate::game::camera::ChartCamera;
use crate::game::chart_view::ChartView;
use crate::scene::Scene;
use crate::shaded_mesh::ShadedMesh;

#[derive(PartialEq, PartialOrd)]
enum TestBg {
    None,
    Ksm,
    Sdvx,
}

impl ToString for TestBg {
    fn to_string(&self) -> String {
        match self {
            TestBg::None => "None".to_string(),
            TestBg::Ksm => "KSM".to_string(),
            TestBg::Sdvx => "SDVX".to_string(),
        }
    }
}

pub struct CameraTest {
    camera: ChartCamera,
    close: bool,
    track_shader: ShadedMesh,
    crit_line: three_d::Gm<Mesh, ColorMaterial>,
    gizmo: three_d::Axes,
    ksm_bg: Gm<Rectangle, ColorMaterial>,
    sdvx_bg: Gm<Rectangle, ColorMaterial>,
    bg: TestBg,
}

impl CameraTest {
    pub fn new(service_provider: ServiceProvider, skin_folder: PathBuf) -> Self {
        let context = service_provider
            .get_required::<three_d::Context>()
            .as_ref()
            .clone();

        let mut shader_folder = skin_folder.clone();
        let mut texture_folder = skin_folder.clone();
        shader_folder.push("shaders");
        texture_folder.push("textures");
        texture_folder.push("dummy.png");

        let ksm: CpuTexture =
            three_d_asset::io::load_and_deserialize(texture_folder.with_file_name("ksm.jpg"))
                .expect("ksm.jpg not found");

        let ksm = three_d::Gm::new(
            Rectangle::new(&context, Vec2::zero(), Rad::zero(), 1.0, 1.0),
            three_d::ColorMaterial {
                texture: Some(Texture2DRef {
                    texture: Arc::new(three_d::Texture2D::new(&context, &ksm)),
                    transformation: Mat3::from_nonuniform_scale(1.0, 1.0),
                }),
                color: three_d::Srgba::WHITE,
                ..Default::default()
            },
        );

        let sdvx: CpuTexture =
            three_d_asset::io::load_and_deserialize(texture_folder.with_file_name("sdvx.jpg"))
                .expect("sdvx.jpg not found");

        let sdvx = three_d::Gm::new(
            Rectangle::new(&context, Vec2::zero(), Rad::zero(), 1.0, 1.0),
            three_d::ColorMaterial {
                texture: Some(Texture2DRef {
                    texture: Arc::new(three_d::Texture2D::new(&context, &sdvx)),
                    transformation: Mat3::from_nonuniform_scale(1.0, 1.0),
                }),
                color: three_d::Srgba::WHITE,
                ..Default::default()
            },
        );

        let mut track_shader =
            ShadedMesh::new(&context, "track", &shader_folder).expect("Failed to load shader:");
        track_shader.set_data_mesh(&crate::game::graphics::xy_rect(
            Vec3::zero(),
            vec2(1.0, ChartView::TRACK_LENGTH * 2.0),
        ));

        track_shader.set_param("lCol", Srgba::BLUE);
        track_shader.set_param("rCol", Srgba::RED);

        track_shader
            .use_texture(
                "mainTex",
                texture_folder.with_file_name("track.png"),
                (false, false),
            )
            .expect("Failed to set texture uniform");
        let mat = three_d::ColorMaterial::new_opaque(
            &context,
            &three_d::material::CpuMaterial {
                albedo: Srgba::RED,
                ..Default::default()
            },
        );
        let mut crit_line = three_d::Mesh::new(&context, &three_d::CpuMesh::square());
        crit_line.set_transformation(Matrix4::from_nonuniform_scale(1.0, 0.03, 1.0));
        let crit_line = Gm::new(crit_line, mat);
        Self {
            camera: ChartCamera::new(),
            close: false,
            track_shader,
            crit_line,
            gizmo: three_d::Axes::new(&context, 0.05, 0.5),
            ksm_bg: ksm,
            sdvx_bg: sdvx,
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
        dt: f64,
        td_context: &three_d::Context,
        target: &mut three_d::RenderTarget,
        viewport: three_d::Viewport,
    ) {
        self.camera
            .update(vec2(viewport.width as f32, viewport.height as f32));
        let td_camera: Camera = Camera::from(&self.camera);

        match self.bg {
            TestBg::None => {}
            TestBg::Ksm => {
                self.ksm_bg
                    .set_size(viewport.width as f32, viewport.height as f32);
                self.ksm_bg.set_center(vec2(
                    viewport.width as f32 / 2.0,
                    viewport.height as f32 / 2.0,
                ));

                let mut new_2d = Camera::new_2d(viewport);
                new_2d.disable_tone_and_color_mapping();
                target.render(&new_2d, &[&self.ksm_bg], &[]);
            }
            TestBg::Sdvx => {
                self.sdvx_bg
                    .set_size(viewport.width as f32, viewport.height as f32);
                self.sdvx_bg.set_center(vec2(
                    viewport.width as f32 / 2.0,
                    viewport.height as f32 / 2.0,
                ));
                let mut new_2d = Camera::new_2d(viewport);
                new_2d.disable_tone_and_color_mapping();
                target.render(&new_2d, &[&self.sdvx_bg], &[]);
            }
        }

        target.render(&td_camera, [&self.track_shader], &[]);
        target.render(&td_camera, [&self.crit_line], &[]);
        target.render(&td_camera, [&self.gizmo], &[]);
    }
}
