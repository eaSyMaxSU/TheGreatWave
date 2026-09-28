use std::fs;
use std::path::Path;

#[test]
fn native_fixtures_match_legacy_and_round_trip() {
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
        let json =
            fs::read_to_string(fixtures.join("legacy").join(format!("{name}.json5"))).unwrap();
        let native = fs::read_to_string(fixtures.join(format!("{name}.tgw"))).unwrap();
        let expected = tgw::render(&json).unwrap();
        assert_eq!(
            tgw::render(&native).unwrap(),
            expected,
            "native fixture {name}"
        );
        let converted = tgw::to_tgw(&json).unwrap();
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
fn native_format_can_be_selected_explicitly() {
    let source = "clk: P...\ndata: x3.4 => ready \"two words\" ; phase=.25\n";
    let mut out = Vec::new();
    tgw::render_with_format(source, &mut out, 2, tgw::InputFormat::Tgw).unwrap();
    assert!(String::from_utf8(out.clone())
        .unwrap()
        .contains("two words"));
    assert!(tgw::render_with_format(source, &mut out, 0, tgw::InputFormat::Json5).is_err());
    assert!(out.is_empty());
    assert!(tgw::render("\u{feff}clk: p...\n").is_ok());
}

#[test]
fn conversion_preserves_unicode_special_characters_and_empty_diagrams() {
    for source in [
        r#"{signal:[{name:'a:b; 🌊',wave:'23',data:['a # b','line\n\u0001\uD83C\uDF0A']}]}"#,
        "{signal:[]}",
        "{signal:[{wave:'p...'}],head:{tick:0,every:-2}}",
        "{signal:[['outer',['inner',{name:'a',wave:'01'}]]],config:{hbounds:[1,2]},head:{tick:'0.25 0.1'}}",
        "{signal:[{name:'p',wave:['pw',{d:'M0,0 L1,1'}]}]}",
        "{signal:[{node:'a...b'}],edge:['a<->b label']}",
    ] {
        let converted = tgw::to_tgw(source).unwrap();
        assert_eq!(tgw::render(&converted).unwrap(), tgw::render(source).unwrap(), "{converted}");
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
