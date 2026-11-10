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
    let c = ksh("t=120", "beat=4/4\n2000|00|--\n2000|00|--\n1000|00|--\n0000|00|--");
    let bt = &c.note.bt[0];
    assert_eq!(bt.len(), 2);
    assert_eq!((bt[0].y, bt[0].l), (0, 480));
    assert_eq!((bt[1].y, bt[1].l), (480, 0));
}

#[test]
fn ksh_long_fx_followed_by_chip() {
    let c = ksh("t=120", "beat=4/4\n0000|10|--\n0000|10|--\n0000|20|--\n0000|00|--");
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
    assert_eq!((c.beat.time_sig[0].1 .0, c.beat.time_sig[0].1 .1), (4, 4));
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
    assert_eq!(unknown.option.get("foo:bar:baz"), Some(&vec![(0, "1".to_string())]));
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
        retrigger[0].value().unwrap().get("wave_length").map(String::as_str),
        Some("1/8")
    );
    let tapestop = &long_event.get("tapestop").expect("tapestop")[1];
    assert_eq!(
        tapestop[0].value().unwrap().get("speed").map(String::as_str),
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
