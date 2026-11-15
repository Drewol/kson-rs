use serde_test::Token;

use crate::{
    do_curve,
    parameter::{self, EffectFloat, EffectFreq, EffectParameterValue},
};

mod serializer;

#[test]
fn curves() {
    for i in 0..=100 {
        let i = i as f64 / 100.0;

        assert!(do_curve(i, 0.0, 0.5).is_finite());
        assert!(do_curve(i, 1.0, 0.5).is_finite());
        assert!(do_curve(i, 0.5, 1.0).is_finite());
        assert!(do_curve(i, 0.5, 0.0).is_finite());
    }
}

#[test]
fn effect_param() {
    let mut param = parameter::EffectParameter {
        on: Some(EffectParameterValue::Freq(
            EffectFreq::Khz(10.0)..=EffectFreq::Khz(20.0),
        )),
        off: EffectParameterValue::Freq(EffectFreq::Hz(500)..=EffectFreq::Hz(500)),
        v: 0.0_f32,
        shape: parameter::InterpolationShape::Logarithmic,
        ty: Default::default(),
    };

    serde_test::assert_tokens(&param, &[Token::Str("500Hz>10kHz-20kHz")]);
    param.shape = parameter::InterpolationShape::Linear;
    param.on = None;
    param.off = EffectParameterValue::Filename("e9fda14b-d635-4cd8-8c7a-ca12f8d9b78a".to_string());

    serde_test::assert_tokens(
        &param,
        &[Token::Str("e9fda14b-d635-4cd8-8c7a-ca12f8d9b78a")],
    );

    param.off = EffectParameterValue::Sample(100..=100);
    serde_test::assert_tokens(&param, &[Token::Str("100samples")]);
    // "OnMin-OnMax": Off inherits the OnMin value
    param.off = EffectParameterValue::Sample(100..=100);
    param.on = Some(EffectParameterValue::Sample(100..=1000));
    serde_test::assert_de_tokens(&param, &[Token::Str("100samples-1000samples")]);
    serde_test::assert_ser_tokens(&param, &[Token::Str("100samples>100samples-1000samples")]);
    param.on = None;

    param.off = EffectParameterValue::Length(
        EffectFloat::Fraction(1, 2)..=EffectFloat::Fraction(1, 2),
        true,
    );
    serde_test::assert_tokens(&param, &[Token::Str("1/2")]);

    param.off = EffectParameterValue::Switch(false..=false);
    param.on = Some(EffectParameterValue::Switch(false..=true));
    serde_test::assert_tokens(&param, &[Token::Str("off>off-on")]);
}

fn ksh(header: &str, body: &str) -> crate::Chart {
    let data = format!("{header}\nver=171\n--\n{body}\n--\n");
    crate::Chart::from_ksh(&data).expect("Failed to parse ksh")
}

#[test]
fn ksh_long_bt_followed_by_chip() {
    let c = ksh(
        "t=120",
        "beat=4/4\n2000|00|--\n2000|00|--\n1000|00|--\n0000|00|--",
    );
    let bt = &c.note.bt[0];
    assert_eq!(bt.len(), 2);
    assert_eq!((bt[0].y, bt[0].l), (0, 480));
    assert_eq!((bt[1].y, bt[1].l), (480, 0));
}

#[test]
fn ksh_long_fx_followed_by_chip() {
    let c = ksh(
        "t=120",
        "beat=4/4\n0000|10|--\n0000|10|--\n0000|20|--\n0000|00|--",
    );
    let fx = &c.note.fx[0];
    assert_eq!(fx.len(), 2);
    assert_eq!((fx[0].y, fx[0].l), (0, 480));
    assert_eq!((fx[1].y, fx[1].l), (480, 0));
}

#[test]
fn ksh_defaults_and_option_values() {
    let c = crate::Chart::from_ksh("title=1+1=2\n--\n0000|00|--\n--\n").unwrap();
    assert_eq!(c.meta.title, "1+1=2");
    assert_eq!(c.beat.bpm, vec![(0, 120.0)]);
    assert_eq!(c.beat.time_sig[0].0, 0);
    assert_eq!(c.beat.time_sig[0].1, crate::TimeSignature::default());
    assert_eq!(c.meta.difficulty, 0);
    // No "ver": legacy chart
    assert_eq!(c.compat.as_ref().unwrap().ksh_version, "100");
    assert!((c.audio.bgm.vol - 0.6).abs() < 1e-9);
    // Default laser effect is the peaking filter
    assert_eq!(
        c.audio.audio_effect.laser.pulse_event.get("peaking_filter"),
        Some(&vec![0])
    );
}

#[test]
fn ksh_unknown_options_and_deprecated_tilt() {
    let c = ksh(
        "t=120\ndifficulty=gravity",
        "foo:bar:baz=1\ntilt=big\n0000|00|--\ntilt=keep\n0000|00|--",
    );
    assert_eq!(c.meta.difficulty, 3);
    let unknown = &c.compat.as_ref().unwrap().ksh_unknown;
    assert_eq!(
        unknown.option.get("foo:bar:baz"),
        Some(&vec![(0, "1".to_string())])
    );
    assert!(matches!(
        c.camera.tilt[0].1,
        crate::camera::TiltValue::Named(crate::camera::NamedTiltValue::Bigger)
    ));
    assert!(matches!(
        c.camera.tilt[1].1,
        crate::camera::TiltValue::Named(crate::camera::NamedTiltValue::KeepBigger)
    ));
}

#[test]
fn ksh_swing() {
    let c = ksh("t=120", "0000|00|--S<192;100;3;1\n0000|00|--S>96");
    let swing = &c.camera.cam.pattern.laser.slam_event.swing;
    assert_eq!(swing.len(), 2);
    assert_eq!((swing[0].0, swing[0].1, swing[0].2), (0, -1, 960));
    assert_eq!(
        (swing[0].3.scale, swing[0].3.repeat, swing[0].3.decay_order),
        (100.0, 3, 1)
    );
    assert_eq!((swing[1].1, swing[1].2), (1, 480));
    assert_eq!(
        (swing[1].3.scale, swing[1].3.repeat, swing[1].3.decay_order),
        (250.0, 3, 2)
    );
}

#[test]
fn ksh_legacy_and_builtin_fx() {
    let c = ksh(
        "t=120",
        "0000|S0|--\n0000|S0|--\nfx-r=TapeStop;20\n0000|01|--\n0000|00|--",
    );
    let long_event = &c.audio.audio_effect.fx.long_event;
    let retrigger = &long_event.get("retrigger").expect("retrigger")[0];
    assert_eq!(
        retrigger[0]
            .value()
            .unwrap()
            .get("wave_length")
            .map(String::as_str),
        Some("1/8")
    );
    let tapestop = &long_event.get("tapestop").expect("tapestop")[1];
    assert_eq!(
        tapestop[0]
            .value()
            .unwrap()
            .get("speed")
            .map(String::as_str),
        Some("20%")
    );
    // Builtin effects are resolved without a definition
    assert!(c.audio.audio_effect.fx.def.is_empty());
    let tracks = c.get_effect_tracks();
    assert!(tracks
        .iter()
        .any(|t| matches!(t.effect, crate::effects::AudioEffect::Retrigger(_))));
    assert!(tracks
        .iter()
        .any(|t| matches!(t.effect, crate::effects::AudioEffect::Tapestop(_))));
}

#[test]
fn effect_param_negative_values() {
    use parameter::EffectParameter;
    let p: EffectParameter<f32> = "-6".parse().unwrap();
    assert_eq!(p.off, EffectParameterValue::Float(-6.0..=-6.0));
    let p: EffectParameter<f32> = "0>-12".parse().unwrap();
    assert_eq!(p.on, Some(EffectParameterValue::Float(-12.0..=-12.0)));
    let p: EffectParameter<f32> = "-12--6".parse().unwrap();
    assert_eq!(p.on, Some(EffectParameterValue::Float(-12.0..=-6.0)));
    let p: EffectParameter<f32> = "-8.0dB".parse().unwrap();
    assert_eq!(p.off, EffectParameterValue::Decibel(-8.0..=-8.0));
    assert_eq!(p.to_string(), "-8dB");
    // OnMin-OnMax: Off inherits OnMin
    let p: EffectParameter<f32> = "50%-100%".parse().unwrap();
    assert_eq!(p.off, EffectParameterValue::Rate(0.5..=0.5));
    assert_eq!(p.interpolate(1.0, false), 0.5);
    assert_eq!(p.interpolate(1.0, true), 1.0);
}

#[test]
fn kson_spec_fields() {
    let json = r#"{
        "format_version": 1,
        "meta": {"title": "t", "artist": "a", "chart_author": "c", "difficulty": "gravity", "level": 1, "disp_bpm": "120"},
        "beat": {"bpm": [[0, 120.0]]},
        "gauge": {"total": 120},
        "audio": {"key_sound": {"laser": {}}, "audio_effect": {"laser": {"def": [["f", {"type": "bitcrusher"}]]}}},
        "camera": {"cam": {"body": {"zoom_side": [[0, 10.0]], "center_split": [[0, 5.0]], "rotation_deg": [[0, 90.0]]}}}
    }"#;
    let chart: crate::Chart = serde_json::from_str(json).expect("Failed to read kson");
    assert_eq!(chart.meta.difficulty, 3);
    assert_eq!(chart.gauge.as_ref().unwrap().total, 120);
    assert_eq!(chart.camera.cam.body.zoom_side.len(), 1);
    assert_eq!(chart.camera.cam.body.center_split.len(), 1);
    assert_eq!(chart.camera.cam.body.rotation_deg.len(), 1);
    assert_eq!(chart.audio.key_sound.laser.vol, vec![(0, 0.5)]);
    assert_eq!(chart.audio.bgm.preview.duration, 15000);
    assert!(matches!(
        chart.audio.audio_effect.laser.def[0].1,
        crate::effects::AudioEffect::BitCrusher(_)
    ));

    let out = serde_json::to_string(&chart).unwrap();
    assert!(!out.contains("null"), "kson must not contain null: {out}");
    assert!(out.contains("\"bitcrusher\""));
    assert!(out.contains("\"zoom_side\""));

    let from_ksh = crate::Chart::from_ksh(include_str!("tests/assets/Gram_ex.ksh")).unwrap();
    let out = serde_json::to_string(&from_ksh).unwrap();
    assert!(!out.contains("null"), "kson must not contain null");
}

#[test]
fn effect_param_typed_formats() {
    use parameter::{EffectParameter, ParamType};
    let p = |s: &str, ty| EffectParameter::<f32>::parse_typed(s, ty);

    // length: plain float is a fraction of a measure, ms/s are absolute
    let bpm = 120.0;
    let d = |s: &str| p(s, ParamType::Length).unwrap().to_duration(bpm, 0.0, false);
    assert_eq!(d("1/4").as_secs_f32(), 0.5);
    assert_eq!(d("0.5").as_secs_f32(), 1.0);
    assert_eq!(d("2").as_secs_f32(), 4.0);
    assert_eq!(d("100ms").as_secs_f32(), 0.1);
    assert_eq!(d("1s").as_secs_f32(), 1.0);
    assert_eq!(p("0", ParamType::Length).unwrap().to_duration(bpm, 0.0, false), std::time::Duration::ZERO);
    assert!(p("1/0", ParamType::Length).is_err());
    assert!(p("2/3", ParamType::Length).is_err());
    assert!(p("0ms", ParamType::Length).is_err());
    assert!(p("-1", ParamType::Length).is_err());
    // A range cannot mix tempo synced and absolute values
    assert!(p("1/4-100ms", ParamType::Length).is_err());

    // rate
    assert!(p("101%", ParamType::Rate).is_err());
    assert!(p("1.5", ParamType::Rate).is_err());
    assert_eq!(p("1/2", ParamType::Rate).unwrap().interpolate(0.0, false), 0.5);
    // Each side of a range is parsed as the parameter type
    assert!(p("50%-100samples", ParamType::Rate).is_err());

    // freq
    assert!(p("5Hz", ParamType::Freq).is_err());
    assert!(p("21kHz", ParamType::Freq).is_err());
    assert!(p("1000", ParamType::Freq).is_err());
    assert!((p("2kHz", ParamType::Freq).unwrap().interpolate(0.0, false) - 2000.0).abs() < 0.01);

    // sample, switch, dB, float formats
    assert!(p("44101samples", ParamType::Sample).is_err());
    assert!(p("100", ParamType::Sample).is_err());
    assert!(p("yes", ParamType::Switch).is_err());
    assert!(p("-8", ParamType::Decibel).is_err());
    assert!(p("1e3", ParamType::Float).is_err());
    assert!(p("+1", ParamType::Float).is_err());
    assert_eq!(p("-12--6", ParamType::Float).unwrap().interpolate(1.0, true), -6.0);

    // pitch: quantized (floored) only when written without '.'
    let pitch = p("0>-12-12", ParamType::Pitch).unwrap();
    assert_eq!(pitch.interpolate(0.45, true), -2.0);
    let pitch = p("0>-12.0-12.0", ParamType::Pitch).unwrap();
    assert!((pitch.interpolate(0.45, true) + 1.2).abs() < 1e-4);
    assert_eq!(pitch.to_string(), "0>-12.0-12.0");
    assert_eq!(p("0>12", ParamType::Pitch).unwrap().to_string(), "0>12");

    // int transition values are floored
    let int = EffectParameter::<i64>::parse_typed("-2-0", ParamType::Int).unwrap();
    assert_eq!(int.interpolate(0.45, true), -2);
}

#[test]
fn effect_derive_typed_and_aliases() {
    use crate::effects::{AudioEffect, Effect};
    let wobble = AudioEffect::builtin("wobble").unwrap();
    // Invalid values are ignored
    let derived = wobble.derive("freq_1", "500");
    assert_eq!(derived, wobble);
    // serde aliases are accepted
    let AudioEffect::Wobble(w) = wobble.derive("lo_freq", "1kHz") else {
        unreachable!()
    };
    assert!((w.freq_1.interpolate(0.0, false) - 1000.0).abs() < 0.01);
    // Filters accept freq_max and bandwidth
    let AudioEffect::PeakingFilter(f) = AudioEffect::builtin("peaking_filter")
        .unwrap()
        .derive("freq_max", "4kHz")
        .derive("bandwidth", "2.0")
    else {
        unreachable!()
    };
    assert!((f.freq_max.interpolate(0.0, false) - 4000.0).abs() < 0.01);
    assert_eq!(f.bandwidth.interpolate(0.0, false), 2.0);
}

#[test]
fn effect_builtin_defaults_round_trip() {
    use crate::effects::AudioEffect;
    for name in [
        "retrigger",
        "gate",
        "flanger",
        "pitch_shift",
        "bitcrusher",
        "phaser",
        "wobble",
        "tapestop",
        "echo",
        "sidechain",
        "peaking_filter",
        "high_pass_filter",
        "low_pass_filter",
    ] {
        let effect = AudioEffect::builtin(name).unwrap();
        let json = serde_json::to_string(&effect).unwrap();
        for non_spec in ["pitch_quantize", "\"delay\":\"0", "Undefined"] {
            assert!(!json.contains(non_spec), "{name}: {json}");
        }
        let back: AudioEffect = serde_json::from_str(&json).unwrap();
        assert_eq!(back, effect, "{name}: {json}");
    }
}

#[test]
fn legacy_sidechain_release_time() {
    use crate::effects::AudioEffect;
    let chart = |ver: &str| {
        let data = format!(
            "t=120\nver={ver}\n--\nfx-l=SC\n0000|10|--\n0000|10|--\n0000|00|--\n--\n#define_fx SC type=SideChain\n"
        );
        crate::Chart::from_ksh(&data).unwrap()
    };
    let release_time = |c: &crate::Chart| {
        c.get_effect_tracks()
            .into_iter()
            .find_map(|t| match t.effect {
                AudioEffect::Sidechain(s) => Some(s.release_time.unwrap().to_string()),
                _ => None,
            })
            .expect("sidechain track")
    };
    let legacy = chart("171");
    // Not written to the chart; resolved when the effect is used
    let AudioEffect::Sidechain(def) = &legacy.audio.audio_effect.fx.def[0].1 else {
        unreachable!()
    };
    assert!(def.release_time.is_none());
    assert_eq!(release_time(&legacy), "1/16");
    assert_eq!(release_time(&chart("200")), "1/8");
}
