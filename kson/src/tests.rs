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
    param.off = EffectParameterValue::Sample(100..=1000);
    serde_test::assert_tokens(&param, &[Token::Str("100samples-1000samples")]);

    param.off = EffectParameterValue::Length(
        EffectFloat::Fraction(1, 2)..=EffectFloat::Fraction(1, 2),
        true,
    );
    serde_test::assert_tokens(&param, &[Token::Str("1/2")]);

    param.off = EffectParameterValue::Switch(false..=false);
    param.on = Some(EffectParameterValue::Switch(false..=true));
    serde_test::assert_tokens(&param, &[Token::Str("off>off-on")]);
}
