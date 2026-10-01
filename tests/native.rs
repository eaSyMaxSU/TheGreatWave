use std::fs;
use std::path::Path;

#[test]
fn native_fixtures_round_trip() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "step4",
        "arcs",
        "arcs1",
        "clocks",
        "gaps",
        "bundles",
        "marks",
        "precision",
    ] {
        let native = fs::read_to_string(fixtures.join(format!("{name}.tgw"))).unwrap();
        let expected = tgw::render(&native).unwrap();
        let converted = tgw::to_tgw(&native).unwrap();
        assert_eq!(
            tgw::render(&converted).unwrap(),
            expected,
            "converted {name}"
        );
        assert_eq!(
            tgw::to_tgw(&converted).unwrap(),
            converted,
            "stable format {name}"
        );
    }
}

#[test]
fn indent_and_a_leading_bom() {
    let source = "clk: P...\ndata: x3.4 => ready \"two words\" ; phase=.25\n";
    let mut out = Vec::new();
    tgw::render_opts(source, &mut out, 2).unwrap();
    assert!(String::from_utf8(out).unwrap().contains("two words"));
    assert!(tgw::render("\u{feff}clk: p...\n").is_ok());
}

#[test]
fn conversion_preserves_unicode_special_characters_and_empty_diagrams() {
    for source in [
        "\"a:b; 🌊\": 23 => \"a # b\" \"line\\n\\u0001\\uD83C\\uDF0A\"\n",
        "@empty\n",
        "@tick 0\n@every -2\n: p...\n",
        "@tick 0.25 0.1\n@bounds 1 2\n@group outer\n  @group inner\n    a: 01\n  @end\n@end\n",
        "p: path M0,0 L1,1\n",
        ": ; node=a...b\n@edge a<->b label\n",
    ] {
        let converted = tgw::to_tgw(source).unwrap();
        assert_eq!(
            tgw::render(&converted).unwrap(),
            tgw::render(source).unwrap(),
            "{converted}"
        );
    }
}

#[test]
fn malformed_native_input_is_bounded() {
    let alphabet = [
        ':', ';', '=', '>', '#', '@', '[', ']', '\'', '"', '\\', ' ', '\n', 'p', '.', '0', '波',
    ];
    let mut state = 92761_u64;
    for length in 0..120 {
        for _ in 0..8 {
            let mut source = String::from("@title fuzz\nclk:");
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push(alphabet[(state >> 32) as usize % alphabet.len()]);
            }
            let _ = tgw::render(&source);
        }
    }
}
