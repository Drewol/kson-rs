use std::{collections::HashSet, path::Path, rc::Rc, sync::Arc};

use crate::{config::GameConfig, game::HoldState};

use super::graphics::{self, GlVertex};

pub struct ChartView {
    pub hispeed: f32,
    pub cursor: f64,
    laser_meshes: [Vec<Vec<graphics::GlVertex>>; 2],
    track: CpuMesh,
    distant_button_scale: f32,
    pub state: i32,
}

use anyhow::anyhow;
use kson::KSON_RESOLUTION;
use puffin::{profile_function, profile_scope};
use three_d::{
    vec2, vec3, Blend, ColorMaterial, CpuMesh, DepthTest, Indices, Mat3, RenderStates, Texture2D,
    Vec3,
};
use three_d_asset::Srgba;
impl ChartView {
    pub const TRACK_LENGTH: f32 = 12.0;
    pub const UP: Vec3 = vec3(0.0, 0.0, -1.0);
    pub const TRACK_DIRECTION: Vec3 = vec3(0.0, 1.0, 0.0);
    pub const Z_NEAR: f32 = 0.01;

    pub fn new(skin_root: impl AsRef<Path>, td: &three_d::Context) -> anyhow::Result<Self> {
        let _indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut texure_path = skin_root.as_ref().to_path_buf();
        texure_path.push("textures");
        texure_path.push("file.png");
        td.set_depth_test(three_d::DepthTest::Never);

        let mut textures = three_d_asset::io::load(&[
            texure_path.with_file_name("laser_l.png"),
            texure_path.with_file_name("laser_r.png"),
            texure_path.with_file_name("track.png"),
            texure_path.with_file_name("fxbutton.png"),
            texure_path.with_file_name("button.png"),
        ])?;

        let _laser_texture = Some(Arc::new(Texture2D::new(
            td,
            &textures.deserialize("laser_l")?,
        )));
        let _laser_render_states = RenderStates {
            blend: Blend::ADD,
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        let track_texture = Arc::new(Texture2D::new(td, &textures.deserialize("track")?));

        let _track_mat = Rc::new(ColorMaterial {
            color: Srgba::WHITE,
            texture: Some(three_d::Texture2DRef {
                texture: track_texture,
                transformation: Mat3::from_scale(1.0),
            }),
            render_states: RenderStates {
                depth_test: three_d::DepthTest::Always,
                ..Default::default()
            },
            ..Default::default()
        });

        let track = graphics::xy_rect(vec3(0.0, 0.0, 0.0), vec2(1.0, Self::TRACK_LENGTH * 2.0));
        let _button_render_states = RenderStates {
            depth_test: DepthTest::Always,
            ..Default::default()
        };

        Ok(ChartView {
            distant_button_scale: GameConfig::get().distant_button_scale,
            cursor: 0.0,
            hispeed: 1.0,
            laser_meshes: [Vec::new(), Vec::new()],
            track,
            state: 0,
        })
    }

    pub fn build_laser_meshes(&mut self, chart: &kson::Chart) {
        for i in 0..2 {
            self.laser_meshes[i].clear();
            for section in &chart.note.laser[i] {
                let mut section_verts = Vec::new();
                let w = 1.0 / 6.0;
                let (xoff, track_w) = if section.wide() < 2 {
                    (2.0 / 6.0, 5.0 / 6.0)
                } else {
                    (9.0 / 12.0, 10.0 / 6.0)
                };
                let mut is_first = true;
                for se in section.segments() {
                    let s = se[0];
                    let e = se[1];
                    let mut syoff = 0.0_f32;
                    let mut start_value = s.v as f32 * track_w;

                    if let Some(value) = s.vf {
                        let value = value as f32 * track_w;
                        syoff = KSON_RESOLUTION as f32 / 8.0;
                        graphics::generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            s.ry as f32,
                            w,
                            is_first,
                            false,
                        );
                        start_value = value;
                    }
                    let end_value = e.v as f32 * track_w;
                    let x00 = end_value - w - xoff;
                    let x01 = end_value - xoff;
                    let x10 = start_value - w - xoff;
                    let x11 = start_value - xoff;
                    let y0 = e.ry as f32;
                    let y1 = s.ry as f32 + syoff;

                    section_verts.append(&mut vec![
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y0, 0.0, x01], [1.0, 0.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
                        GlVertex::new([y1, 0.0, x10], [0.0, 1.0]),
                        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
                    ]);
                    is_first = false;
                }
                if let Some(e) = section.last() {
                    if let Some(value) = e.vf {
                        let start_value = e.v as f32 * track_w;
                        let value = value as f32 * track_w;
                        let syoff = KSON_RESOLUTION as f32 / 8.0;
                        graphics::generate_slam_verts(
                            &mut section_verts,
                            start_value,
                            value,
                            syoff,
                            xoff,
                            e.ry as f32,
                            w,
                            is_first,
                            true,
                        );
                    }
                }
                self.laser_meshes[i].push(section_verts);
            }
        }
    }

    pub fn render(
        &mut self,
        chart: &kson::Chart,
        td: &three_d::Context,
        buttons_held: HashSet<usize>,
        mut beam_colors: [[f32; 4]; 6],
    ) -> anyhow::Result<graphics::TrackRenderMeshes> {
        use three_d::prelude::*;
        profile_function!();
        let view_time = self.cursor;
        let view_offset = if view_time < 0.0 {
            chart.ms_to_tick(view_time.abs()) as i64 //will be weird with early bpm changes
        } else {
            0
        };

        td.set_depth_test(three_d::DepthTest::Never);

        let _glow_state = if (0.0_f32 * 8.0).fract() > 0.5 { 2 } else { 3 };
        let view_tick = chart.ms_to_tick(view_time) as i64 - view_offset;
        let view_distance = (KSON_RESOLUTION as f32 * 8.0) / self.hispeed;
        let last_view_tick = view_distance.ceil() as i64 + view_tick;
        let first_view_tick = view_tick - view_distance as i64;
        let y_view_div = view_distance / Self::TRACK_LENGTH;
        let _white_mat = Rc::new(ColorMaterial {
            color: Srgba::WHITE,
            ..Default::default()
        });

        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        #[allow(unused)]
        enum NoteType {
            BtChip,
            BtHold,
            BtHoldActive(usize),
            FxChip,
            FxChipSample,
            FxHold,
            FxHoldActive(usize),
        }
        let mut notes = Vec::new();
        let chip_h = 1.0;

        let _track = self.track.clone();

        {
            profile_scope!("Build notes");
            for i in 0..4 {
                for n in &chart.note.bt[i] {
                    if (n.y as i64) > last_view_tick {
                        break;
                    } else if ((n.y + n.l) as i64) < first_view_tick {
                        continue;
                    }

                    let w = 0.9 / 6.0;
                    let x = 1.5 / 6.0 + (i as f32 / 6.0);
                    let h = if n.l == 0 {
                        chip_h
                    } else {
                        (n.l as f32) / y_view_div
                    };
                    let yoff = (view_tick - n.y as i64) as f32;
                    let y = yoff / y_view_div;
                    let _p = if n.l == 0 { 2 } else { 1 }; //sorting priority
                    notes.push((
                        vec3(x, y, 0.0),
                        vec2(w, h),
                        if n.l > 0 {
                            if (n.y as i64) < view_tick && ((n.y + n.l) as i64) > view_tick {
                                NoteType::BtHoldActive(i)
                            } else {
                                NoteType::BtHold
                            }
                        } else {
                            NoteType::BtChip
                        },
                    ));
                }
            }
            for i in 0..2 {
                for n in &chart.note.fx[i] {
                    if (n.y as i64) > last_view_tick {
                        break;
                    } else if ((n.y + n.l) as i64) < first_view_tick {
                        continue;
                    }
                    let w = 1.0 / 3.0;
                    let x = 1.0 / 3.0 + (1.0 / 3.0) * i as f32;
                    let h = if n.l == 0 {
                        chip_h
                    } else {
                        (n.l as f32) / y_view_div
                    };
                    let yoff = (view_tick - n.y as i64) as f32;
                    let y = yoff / y_view_div;
                    let _p = if n.l == 0 { 3 } else { 0 }; //sorting priority
                    notes.push((
                        vec3(x, y, 0.0),
                        vec2(w, h),
                        if n.l > 0 {
                            if (n.y as i64) < view_tick && ((n.y + n.l) as i64) > view_tick {
                                NoteType::FxHoldActive(i)
                            } else {
                                NoteType::FxHold
                            }
                        } else {
                            NoteType::FxChip
                        },
                    ));
                }
            }
        }

        let notes = {
            profile_scope!("Transform notes");
            notes.iter().map(|n| {
                let distance_scale = match n.2 {
                    NoteType::BtChip | NoteType::FxChip | NoteType::FxChipSample => {
                        ((-n.0.y / Self::TRACK_LENGTH) * self.distant_button_scale).max(1.0)
                    }
                    _ => 1.0,
                };

                (
                    Mat4::from_translation(n.0)
                        * Mat4::from_nonuniform_scale(1.0, -n.1.y * distance_scale, 1.0),
                    n.2,
                )
            })
        };

        let mut fx_hold = vec![];
        let mut bt_hold = vec![];
        let mut fx_chip = vec![];
        let mut bt_chip = vec![];
        let mut lasers = [
            graphics::xy_rect(Vec3::zero(), Vec2::zero()),
            graphics::xy_rect(Vec3::zero(), Vec2::zero()),
            graphics::xy_rect(Vec3::zero(), Vec2::zero()),
            graphics::xy_rect(Vec3::zero(), Vec2::zero()),
        ];

        //Dim FX beams
        beam_colors[4][3] *= 0.5;
        beam_colors[5][3] *= 0.5;

        let lane_beams = [
            (
                Mat4::from_translation(vec3(-1.5 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(1.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[0]),
            ),
            (
                Mat4::from_translation(-vec3(0.5 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(1.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[1]),
            ),
            (
                Mat4::from_translation(vec3(0.5 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(1.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[2]),
            ),
            (
                Mat4::from_translation(vec3(1.5 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(1.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[3]),
            ),
            (
                Mat4::from_translation(-vec3(1.0 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(2.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[4]),
            ),
            (
                Mat4::from_translation(vec3(1.0 / 6.0, 0.0, 0.0))
                    * Mat4::from_nonuniform_scale(2.0 / 6.0, -ChartView::TRACK_LENGTH, 1.0),
                Srgba::from(beam_colors[5]),
            ),
        ];

        {
            profile_scope!("Sort notes");
            for n in notes {
                match n.1 {
                    NoteType::BtChip => bt_chip.push(n.0),
                    NoteType::BtHold => bt_hold.push((n.0, HoldState::Idle)),
                    NoteType::BtHoldActive(lane) => bt_hold.push((
                        n.0,
                        if buttons_held.contains(&lane) {
                            HoldState::Hit
                        } else {
                            HoldState::Miss
                        },
                    )),
                    NoteType::FxChip => fx_chip.push((n.0, false)),
                    NoteType::FxChipSample => fx_chip.push((n.0, true)),
                    NoteType::FxHold => fx_hold.push((n.0, HoldState::Idle)),
                    NoteType::FxHoldActive(side) => fx_hold.push((
                        n.0,
                        if buttons_held.contains(&(side + 4)) {
                            HoldState::Hit
                        } else {
                            HoldState::Miss
                        },
                    )),
                }
            }
        }

        //lasers
        {
            profile_scope!("Lasers");
            for i in 0..2 {
                for (sidx, s) in chart.note.laser[i].iter().enumerate() {
                    let end_y = s.tick()
                        + s.last()
                            .ok_or(anyhow!("Tried to render an empty laser section"))?
                            .ry;
                    if (s.tick() as i64) > last_view_tick {
                        break;
                    } else if (end_y as i64) < first_view_tick {
                        continue;
                    }
                    let vertices = self.laser_meshes[i]
                        .get(sidx)
                        .ok_or(anyhow!("Laser meshes not built correctly"))?;
                    let yoff = (view_tick - s.tick() as i64) as f32;
                    let laser_mesh = CpuMesh {
                        indices: Indices::U32((0u32..(vertices.len() as u32)).collect()),
                        positions: three_d::Positions::F32(
                            vertices
                                .iter()
                                .map(|v| vec3(v.pos.z, (yoff - v.pos.x) / y_view_div, v.pos.y))
                                .collect(),
                        ),
                        uvs: Some(vertices.iter().map(|v| vec2(v.uv.x, v.uv.y)).collect()),
                        ..Default::default()
                    };

                    let active = if view_tick > s.tick() as i64 && view_tick < end_y as i64 {
                        1
                    } else {
                        0
                    };
                    let extending = std::mem::take(&mut lasers[i * 2 + active]);
                    let extended = graphics::extend_mesh(extending, laser_mesh);
                    lasers[i * 2 + active] = extended;
                }
            }
        }
        Ok(graphics::TrackRenderMeshes {
            fx_hold,
            bt_hold,
            fx_chip,
            bt_chip,
            lasers,
            lane_beams,
        })
    }
}
