use kson::effects::ReTrigger;

use crate::param_input::param_editor;

pub trait EffectEditor {
    fn edit(&mut self, ui: &mut eframe::egui::Ui);
}

//Could probably be a macro
impl EffectEditor for kson::effects::AudioEffect {
    fn edit(&mut self, ui: &mut eframe::egui::Ui) {
        match self {
            kson::effects::AudioEffect::ReTrigger(r) => {
                let ReTrigger {
                    update_period,
                    wave_length,
                    rate,
                    update_trigger,
                    mix,
                } = r;

                ui.label("Update Period");
                ui.add(param_editor(update_period, false));
                ui.end_row();

                ui.label("Wave Length");
                ui.add(param_editor(wave_length, false));
                ui.end_row();

                ui.label("Rate");
                ui.add(param_editor(rate, false));
                ui.end_row();

                ui.label("Update Trigger");
                ui.add(param_editor(update_trigger, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::Gate(kson::effects::Gate {
                wave_length,
                rate,
                mix,
            }) => {
                ui.label("Wave Length");
                ui.add(param_editor(wave_length, false));
                ui.end_row();

                ui.label("Rate");
                ui.add(param_editor(rate, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::Flanger(kson::effects::Flanger {
                period,
                delay,
                depth,
                feedback,
                stereo_width,
                vol,
                mix,
            }) => {
                ui.label("period");
                ui.add(param_editor(period, false));
                ui.end_row();

                ui.label("delay");
                ui.add(param_editor(delay, false));
                ui.end_row();

                ui.label("depth");
                ui.add(param_editor(depth, false));
                ui.end_row();

                ui.label("feedback");
                ui.add(param_editor(feedback, false));
                ui.end_row();

                ui.label("stereo_width");
                ui.add(param_editor(stereo_width, false));
                ui.end_row();

                ui.label("vol");
                ui.add(param_editor(vol, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::PitchShift(kson::effects::PitchShift {
                pitch,
                pitch_quantize,
                chunk_size,
                overlap,
                mix,
            }) => {
                ui.label("pitch");
                ui.add(param_editor(pitch, false));
                ui.end_row();

                ui.label("pitch_quantize");
                ui.add(param_editor(pitch_quantize, false));
                ui.end_row();

                ui.label("chunk_size");
                ui.add(param_editor(chunk_size, false));
                ui.end_row();

                ui.label("overlap");
                ui.add(param_editor(overlap, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::BitCrusher(kson::effects::BitCrusher {
                reduction,
                mix,
            }) => {
                ui.label("Reduction");
                ui.add(param_editor(reduction, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::Phaser(kson::effects::Phaser {
                period,
                stage,
                lo_freq,
                hi_freq,
                q,
                feedback,
                stereo_width,
                mix,
            }) => {
                ui.label("period");
                ui.add(param_editor(period, false));
                ui.end_row();

                ui.label("stage");
                ui.add(param_editor(stage, false));
                ui.end_row();

                ui.label("lo_freq");
                ui.add(param_editor(lo_freq, false));
                ui.end_row();

                ui.label("hi_freq");
                ui.add(param_editor(hi_freq, false));
                ui.end_row();

                ui.label("q");
                ui.add(param_editor(q, false));
                ui.end_row();

                ui.label("feedback");
                ui.add(param_editor(feedback, false));
                ui.end_row();

                ui.label("stereo_width");
                ui.add(param_editor(stereo_width, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::Wobble(kson::effects::Wobble {
                wave_length,
                lo_freq,
                hi_freq,
                q,
                mix,
            }) => {
                ui.label("Wave Length");
                ui.add(param_editor(wave_length, false));
                ui.end_row();

                ui.label("lo_freq");
                ui.add(param_editor(lo_freq, false));
                ui.end_row();

                ui.label("hi_freq");
                ui.add(param_editor(hi_freq, false));
                ui.end_row();

                ui.label("q");
                ui.add(param_editor(q, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::TapeStop(kson::effects::TapeStop {
                speed,
                trigger,
                mix,
            }) => {
                ui.label("Speed");
                ui.add(param_editor(speed, false));
                ui.end_row();

                ui.label("Trigger");
                ui.add(param_editor(trigger, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::Echo(kson::effects::Echo {
                update_period,
                wave_length,
                update_trigger,
                feedback_level,
                mix,
            }) => {
                ui.label("update_period");
                ui.add(param_editor(update_period, false));
                ui.end_row();

                ui.label("wave_length");
                ui.add(param_editor(wave_length, false));
                ui.end_row();

                ui.label("update_trigger");
                ui.add(param_editor(update_trigger, false));
                ui.end_row();

                ui.label("feedback_level");
                ui.add(param_editor(feedback_level, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::SideChain(kson::effects::SideChain {
                period,
                hold_time,
                attack_time,
                release_time,
                ratio,
            }) => {
                ui.label("period");
                ui.add(param_editor(period, false));
                ui.end_row();

                ui.label("hold_time");
                ui.add(param_editor(hold_time, false));
                ui.end_row();

                ui.label("attack_time");
                ui.add(param_editor(attack_time, false));
                ui.end_row();

                ui.label("release_time");
                ui.add(param_editor(release_time, false));
                ui.end_row();

                ui.label("Ratio");
                ui.add(param_editor(ratio, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::AudioSwap(swa) => {
                ui.label("Filename");
                ui.text_edit_singleline(swa);
                ui.end_row();
            }
            kson::effects::AudioEffect::HighPassFilter(kson::effects::HighPassFilter {
                v,
                freq,
                q,
                delay,
                mix,
            }) => {
                ui.label("v");
                ui.add(param_editor(v, false));
                ui.end_row();

                ui.label("freq");
                ui.add(param_editor(freq, false));
                ui.end_row();

                ui.end_row();

                ui.label("q");
                ui.add(param_editor(q, false));
                ui.end_row();

                ui.label("delay");
                ui.add(param_editor(delay, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::LowPassFilter(kson::effects::LowPassFilter {
                v,
                freq,
                q,
                delay,
                mix,
            }) => {
                ui.label("v");
                ui.add(param_editor(v, false));
                ui.end_row();

                ui.label("freq");
                ui.add(param_editor(freq, false));
                ui.end_row();

                ui.end_row();

                ui.label("q");
                ui.add(param_editor(q, false));
                ui.end_row();

                ui.label("delay");
                ui.add(param_editor(delay, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
            kson::effects::AudioEffect::PeakingFilter(kson::effects::PeakingFilter {
                v,
                freq,
                q,
                delay,
                mix,
                gain,
            }) => {
                ui.label("v");
                ui.add(param_editor(v, false));
                ui.end_row();

                ui.label("freq");
                ui.add(param_editor(freq, false));
                ui.end_row();

                ui.label("gain");
                ui.add(param_editor(gain, false));
                ui.end_row();

                ui.label("q");
                ui.add(param_editor(q, false));
                ui.end_row();

                ui.label("delay");
                ui.add(param_editor(delay, false));
                ui.end_row();

                ui.label("Mix");
                ui.add(param_editor(mix, false));
                ui.end_row();
            }
        }
    }
}
