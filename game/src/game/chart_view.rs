use std::{cell::RefCell, collections::HashSet, path::Path};

use crate::{
    config::GameConfig,
    game::{graphics::CpuMesh, HoldState},
    help::RenderContext,
    shaded_mesh::ShadedMesh,
};

use super::graphics::{self, GlVertex, TrackRenderMeshes};

pub struct ChartView {
    pub hispeed: f32,
    pub cursor: f64,
    laser_meshes: [Vec<Vec<graphics::GlVertex>>; 2],
    track: ShadedMesh,
    distant_button_scale: f32,
    key_sounds: [HashSet<u32>; 2],
    note_bases: NoteBases,
    pub render_data: RefCell<TrackRenderMeshes>,
}

/// Untransformed meshes for each note type, every visible note is a transformed copy of one of these
struct NoteBases {
    bt: CpuMesh,
    fx: CpuMesh,
    beam: CpuMesh,
}

impl NoteBases {
    fn new() -> Self {
        Self {
            bt: graphics::xy_rect(vec3(0.0, 0.5, 0.0), vec2(1.0 / 6.0, 1.0)),
            fx: graphics::xy_rect(vec3(0.0, 0.5, 0.0), vec2(2.0 / 6.0, 1.0)),
            beam: graphics::xy_rect(Vec3::ZERO, vec2(1.0, 1.0)),
        }
    }
}

/// Per vertex data for hold notes, `x` = object glow, `y` = hit state
fn hold_vertex_data(state: HoldState, object_glow: f32, hit_state: i32) -> Vec4 {
    let (glow, state) = match state {
        HoldState::Idle => (0.6, 1),
        HoldState::Hit => (object_glow, hit_state),
        HoldState::Miss => (0.3, 0),
    };
    vec4(glow, state as f32, 0.0, 1.0)
}

pub fn gen_laser_subsegment(
    y0: f32,
    y1: f32,
    w: f32,
    start_value: f32,
    end_value: f32,
    xoff: f32,
) -> [GlVertex; 6] {
    let x00 = end_value - w - xoff;
    let x01 = end_value - xoff;
    let x10 = start_value - w - xoff;
    let x11 = start_value - xoff;

    [
        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
        GlVertex::new([y0, 0.0, x01], [1.0, 0.0]),
        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
        GlVertex::new([y0, 0.0, x00], [0.0, 0.0]),
        GlVertex::new([y1, 0.0, x10], [0.0, 1.0]),
        GlVertex::new([y1, 0.0, x11], [1.0, 1.0]),
    ]
}

use anyhow::anyhow;
use glam::{vec2, vec3, vec4, Mat4, Vec3, Vec4};
use kson::{do_curve, score_ticks::ScoreTick, KSON_RESOLUTION};
use puffin::{profile_function, profile_scope};

impl ChartView {
    pub const TRACK_LENGTH: f32 = 16.0;
    pub const UP: Vec3 = vec3(0.0, 0.0, -1.0);
    pub const TRACK_DIRECTION: Vec3 = vec3(0.0, 1.0, 0.0);
    pub const Z_NEAR: f32 = 0.01;

    pub fn new(skin_root: impl AsRef<Path>, td: &RenderContext) -> anyhow::Result<Self> {
        let _indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut texure_path = skin_root.as_ref().to_path_buf();
        texure_path.push("textures");
        texure_path.push("file.png");

        let mut textures = td.load_textures(&[
            texure_path.with_file_name("laser_l.png"),
            texure_path.with_file_name("laser_r.png"),
            texure_path.with_file_name("track.png"),
            texure_path.with_file_name("fxbutton.png"),
            texure_path.with_file_name("button.png"),
        ])?;

        let mut shader_path = skin_root.as_ref().to_path_buf();
        shader_path.push("shaders");

        let track_texture = textures.remove("track").unwrap();

        let mut track = ShadedMesh::new(&td, "track", shader_path)?;

        let base_laser_mesh = CpuMesh {
            indices: Some(vec![]),
            positions: vec![],
            uvs: Some(vec![]),
            ..Default::default()
        };
        let lasers = [
            base_laser_mesh.clone(),
            base_laser_mesh.clone(),
            base_laser_mesh.clone(),
            base_laser_mesh,
        ];

        track.set_data_mesh(&graphics::xy_rect(
            vec3(0.0, 0.0, 0.0),
            vec2(1.0, Self::TRACK_LENGTH * 2.0),
        ));

        Ok(ChartView {
            distant_button_scale: GameConfig::get().distant_button_scale,
            cursor: 0.0,
            hispeed: 1.0,
            laser_meshes: [Vec::new(), Vec::new()],
            track,
            key_sounds: Default::default(),
            note_bases: NoteBases::new(),
            render_data: RefCell::new(TrackRenderMeshes {
                lasers,
                ..Default::default()
            }),
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

                    if s.is_linear() {
                        let end_value = e.v as f32 * track_w;
                        let y0 = e.ry as f32;
                        let y1 = s.ry as f32 + syoff;
                        section_verts.extend(gen_laser_subsegment(
                            y0,
                            y1,
                            w,
                            start_value,
                            end_value,
                            xoff,
                        ));
                    } else {
                        let start_value = s.vf.unwrap_or(s.v) as f32;
                        let len = e.ry - s.ry;
                        let segments = (len / (KSON_RESOLUTION / 32)).max(3); //TODO: Curve quality option
                        let len = len as f32 - syoff;
                        let width = e.v as f32 - start_value;
                        let step = len / segments as f32;

                        for subsegment_index in 0..segments {
                            let s_curve_x = subsegment_index as f64 / segments as f64;
                            let e_curve_x = (subsegment_index + 1) as f64 / segments as f64;

                            let sv = start_value + width * do_curve(s_curve_x, s.a, s.b) as f32;
                            let ev = start_value + width * do_curve(e_curve_x, s.a, s.b) as f32;

                            let y1 = s.ry as f32 + (step * subsegment_index as f32) + syoff;
                            let y0 = if (subsegment_index + 1) == segments {
                                e.ry as f32
                            } else {
                                y1 + step
                            };

                            section_verts.extend(gen_laser_subsegment(
                                y0,
                                y1,
                                w,
                                sv * track_w,
                                ev * track_w,
                                xoff,
                            ));
                        }
                    }
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
    const LASER_SPEED_OFFSET: f32 = 0.9;
    pub fn render(
        &self,
        chart: &kson::Chart,
        hold_ok: impl Fn(usize, u32) -> bool,
        mut beam_colors: [[f32; 4]; 6],
        chip_h: f32,
        object_glow: f32,
        hit_state: i32,
    ) -> anyhow::Result<()> {
        profile_function!();
        let mut data = self.render_data.borrow_mut();
        let TrackRenderMeshes {
            fx_hold,
            bt_hold,
            fx_chip,
            bt_chip,
            lasers,
            lane_beams,
        } = &mut *data;

        fx_hold.clear();
        bt_hold.clear();
        bt_chip.clear();
        fx_chip.clear();
        lane_beams.clear();
        for l in lasers.iter_mut() {
            l.clear();
        }

        //Dim FX beams
        beam_colors[4][3] *= 0.5;
        beam_colors[5][3] *= 0.5;

        let beam_transforms = [
            (-1.5 / 6.0, 1.0 / 6.0),
            (-0.5 / 6.0, 1.0 / 6.0),
            (0.5 / 6.0, 1.0 / 6.0),
            (1.5 / 6.0, 1.0 / 6.0),
            (-1.0 / 6.0, 2.0 / 6.0),
            (1.0 / 6.0, 2.0 / 6.0),
        ];

        for ((x, w), color) in beam_transforms.into_iter().zip(beam_colors) {
            lane_beams.push_transformed(
                &self.note_bases.beam,
                Mat4::from_translation(vec3(x, 0.0, 0.0))
                    * Mat4::from_scale(vec3(w, Self::TRACK_LENGTH, 1.0)),
                Vec4::from(color),
            );
        }

        let chip_h = chip_h.copysign(-1.0);
        let view_time = self.cursor;
        let view_offset = if view_time < 0.0 {
            (view_time / chart.tick_to_ms(1)) as i64
        } else {
            0
        };

        let _glow_state = if (0.0_f32 * 8.0).fract() > 0.5 { 2 } else { 3 };
        let view_tick = chart.ms_to_tick(view_time) as i64 + view_offset;
        let view_distance = (KSON_RESOLUTION as f32 * 8.0) / self.hispeed;
        let last_view_tick = view_distance.ceil() as i64 + view_tick;
        let first_view_tick = view_tick - view_distance as i64;
        let y_view_div = view_distance / -Self::TRACK_LENGTH;
        let laser_y_view_div = y_view_div * Self::LASER_SPEED_OFFSET;

        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        #[allow(unused)]
        enum NoteType {
            BtChip,
            BtHold,
            BtHoldActive(usize, u32),
            FxChip(bool),
            FxHold,
            FxHoldActive(usize, u32),
        }
        let mut notes = Vec::new();

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
                                NoteType::BtHoldActive(i, n.y)
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
                                NoteType::FxHoldActive(i, n.y)
                            } else {
                                NoteType::FxHold
                            }
                        } else {
                            NoteType::FxChip(self.key_sounds[i].contains(&n.y))
                        },
                    ));
                }
            }
        }

        let notes = {
            profile_scope!("Transform notes");
            notes.iter().map(|n| {
                let distance_scale = match n.2 {
                    NoteType::BtChip | NoteType::FxChip(..) => {
                        ((n.0.y / Self::TRACK_LENGTH) * self.distant_button_scale).max(1.0)
                    }
                    _ => 1.0,
                };

                (
                    Mat4::from_translation(n.0)
                        * Mat4::from_scale(vec3(1.0, -n.1.y * distance_scale, 1.0)),
                    n.2,
                )
            })
        };

        {
            profile_scope!("Build note meshes");
            let bases = &self.note_bases;
            let no_sample = vec4(0.0, 0.0, 0.0, 1.0);
            let has_sample = vec4(1.0, 0.0, 0.0, 1.0);
            let hold_state = |hit: bool| {
                if hit {
                    HoldState::Hit
                } else {
                    HoldState::Miss
                }
            };
            for (transform, note_type) in notes {
                match note_type {
                    NoteType::BtChip => bt_chip.push_transformed(&bases.bt, transform, no_sample),
                    NoteType::FxChip(sample) => fx_chip.push_transformed(
                        &bases.fx,
                        transform,
                        if sample { has_sample } else { no_sample },
                    ),
                    NoteType::BtHold => bt_hold.push_transformed(
                        &bases.bt,
                        transform,
                        hold_vertex_data(HoldState::Idle, object_glow, hit_state),
                    ),
                    NoteType::BtHoldActive(lane, y) => bt_hold.push_transformed(
                        &bases.bt,
                        transform,
                        hold_vertex_data(hold_state(hold_ok(lane, y)), object_glow, hit_state),
                    ),
                    NoteType::FxHold => fx_hold.push_transformed(
                        &bases.fx,
                        transform,
                        hold_vertex_data(HoldState::Idle, object_glow, hit_state),
                    ),
                    NoteType::FxHoldActive(side, y) => fx_hold.push_transformed(
                        &bases.fx,
                        transform,
                        hold_vertex_data(
                            hold_state(hold_ok(side + 4, y)),
                            object_glow,
                            hit_state,
                        ),
                    ),
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
                        indices: Some((0..(vertices.len() as u32)).collect()),
                        positions: vertices
                            .iter()
                            .map(|v| vec3(v.pos.z, (yoff - v.pos.x) / laser_y_view_div, v.pos.y))
                            .collect(),

                        uvs: Some(vertices.iter().map(|v| vec2(v.uv.x, v.uv.y)).collect()),
                        ..Default::default()
                    };

                    let active = if view_tick > s.tick() as i64 && view_tick < end_y as i64 {
                        1
                    } else {
                        0
                    };
                    lasers[i * 2 + active].extend(laser_mesh);
                }
            }
        }
        Ok(())
    }
}

impl ChartView {
    pub(crate) fn build_key_sound_map(&mut self, score_ticks: &kson::score_ticks::ScoreTicks) {
        let mut key_sounds: [HashSet<u32>; 2] = Default::default();

        for ele in &score_ticks.ticks {
            let ScoreTick::Chip {
                lane,
                key_sound: Some(_),
            } = ele.tick
            else {
                continue;
            };

            if lane < 4 {
                continue;
            }

            key_sounds[lane - 4].insert(ele.y);
        }

        self.key_sounds = key_sounds;
    }
}
