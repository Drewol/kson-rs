use assert_json_diff::assert_json_include;

/// Parses a ksh and compares it to a kson converted by libkson
#[test]
fn ksh_parser() {
    let in_ksh = include_str!("assets/Gram_ex.ksh");
    let in_kson = include_str!("assets/Gram_ex.json");
    let parsed_ksh = crate::Chart::from_ksh(in_ksh).expect("Failed to parse ksh");
    let mut parsed_kson: serde_json::Value =
        serde_json::from_str(in_kson).expect("Failed to deserialize json");

    serde_json::from_str::<crate::Chart>(in_kson).expect("Failed to read kson");

    let ksh_json = serde_json::to_value(parsed_ksh).expect("Failed to serialize parsed ksh");

    // Editor field is allowed to differ
    parsed_kson
        .as_object_mut()
        .expect("Kson was not an object")
        .remove("editor");

    assert_json_include!(actual: ksh_json, expected: parsed_kson);
}
