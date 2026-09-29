use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(name: &str) -> String {
    let path = root().join("tests/fixtures").join(format!("{name}.tgw"));
    fs::read_to_string(path).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn render_fixture(name: &str) -> String {
    tgw::render(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn tags(svg: &str, name: &str) -> usize {
    svg.matches(&format!("<{name}")).count()
}

#[test]
fn fixtures_render() {
    let step = render_fixture("step4");
    assert!(step.contains(">clk<"));
    assert!(step.contains(">head<"));
    assert!(step.contains(">body<"));
    assert!(step.contains(">tail<"));
    assert!(step.contains("url(#tgw-"));
    assert!(!step.contains("<use"));

    let arcs = render_fixture("arcs");
    assert!(arcs.contains(">t1<"));
    assert!(arcs.contains("time 3"));
    assert!(arcs.contains("some text"));

    let arcs1 = render_fixture("arcs1");
    assert!(arcs1.contains(">Reset<"));
    assert!(arcs1.contains("T &gt; 45s") || arcs1.contains("T > 45s"));

    let clocks = render_fixture("clocks");
    assert!(clocks.contains(">pclk<"));
    assert!(clocks.contains(">Nclk<"));
    assert!(clocks.contains("-k0\""));

    let gaps = render_fixture("gaps");
    assert!(gaps.contains(">Acknowledge<"));
    assert!(gaps.contains(">data<"));

    let bundles = render_fixture("bundles");
    assert!(bundles.contains(">Master<"));
    assert!(bundles.contains(">Slave<"));
    assert!(bundles.contains(">A1<"));
    assert!(bundles.contains(">Q2<"));
    assert!(bundles.contains("stroke=\"#0041c4\""));

    let marks = render_fixture("marks");
    assert!(marks.contains("Timing marks"));
    assert!(marks.contains("Figure 100"));
    assert!(marks.contains(">0<"));
    assert!(marks.contains(">9<"));
    assert!(marks.contains("-gd\""));
}

const SNAPSHOTS: &[&str] = &[
    "step4",
    "arcs",
    "arcs1",
    "clocks",
    "gaps",
    "bundles",
    "marks",
    "precision",
    "alphabet",
    "ticks",
    "spans",
    "markup",
    "unicode",
    "crop",
    "paths",
    "empty",
    "groups",
];

#[test]
fn snapshots() {
    for name in SNAPSHOTS {
        let svg = render_fixture(name);
        let path = root().join("tests/fixtures").join(format!("{name}.svg"));
        if std::env::var_os("UPDATE_SNAPSHOTS").as_deref() == Some(std::ffi::OsStr::new("1")) {
            fs::write(&path, &svg).unwrap();
        } else {
            let expected = fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "read {}: {e}; update explicitly with UPDATE_SNAPSHOTS=1",
                    path.display()
                )
            });
            assert_eq!(expected, svg, "snapshot {name}");
        }
        assert_well_formed(name, &svg);
    }
}

#[test]
fn edge_case_pictures_keep_their_marks() {
    let alphabet = render_fixture("alphabet");
    for name in ["p", "n", "P", "N", "levels", "hiz", "colors", "sub", "gap"] {
        assert!(alphabet.contains(&format!(">{name}<")), "{name}");
    }
    assert!(alphabet.contains("-xh\""));
    assert!(alphabet.contains("-k0\""));

    let ticks = render_fixture("ticks");
    assert!(ticks.contains(">idle<"));
    assert!(ticks.contains(">reply<"));
    assert!(
        !ticks.contains(">request<"),
        "foot-every drops the middle label"
    );
    assert!(ticks.contains(">0.5<") || ticks.contains(">1.0<"));
    assert!(!ticks.contains("-gd\""), "grid off");

    let spans = render_fixture("spans");
    assert!(spans.contains(">pulse<"));
    assert!(spans.contains("arrowhead"));
    // period=2 makes each symbol two cycles; phase=-1.5 delays by 60px and
    // phase=0.75 advances by 30px.
    assert!(spans.contains("translate(60)"));
    assert!(spans.contains("translate(-30)") || spans.contains("translate(-10)"));
    assert!(dimension(&spans, "width") > dimension(&render_fixture("crop"), "width"));

    let markup = render_fixture("markup");
    assert!(markup.contains("A &amp; B &lt; C &gt;"));
    assert!(markup.contains("a &lt; b"));
    assert!(markup.contains("&#10;"));
    assert!(markup.contains("T &gt; 1 &amp; 2"));

    let unicode = render_fixture("unicode");
    assert!(unicode.contains("时钟"));
    assert!(unicode.contains("就绪"));
    assert!(unicode.contains("完成"));

    let crop = render_fixture("crop");
    assert!(
        dimension(&crop, "width") < 320.0,
        "cropped {}",
        dimension(&crop, "width")
    );
    assert!(crop.contains(">b<") && crop.contains(">f<"));
    assert!(
        !crop.contains(">a<"),
        "the value before the window is cropped away"
    );

    let paths = render_fixture("paths");
    assert!(paths.contains("C1.5,0"));
    assert!(paths.contains("A0.5,0.5"));

    let empty = render_fixture("empty");
    assert!(empty.contains(">Nothing here<"));
    assert!(empty.contains(">0 signal lanes<") || empty.contains("0 signal lanes"));
    assert!(!empty.contains("<path"));

    let groups = render_fixture("groups");
    assert!(groups.contains(">Inner<"));
    assert!(groups.contains("very long signal name"));
    assert!(groups.contains("rotate(270)"));
}

fn assert_well_formed(name: &str, svg: &str) {
    assert!(svg.starts_with("<svg "), "{name}");
    assert!(svg.ends_with("</svg>"), "{name}");
    assert!(!svg.contains("</svg "), "{name} rewritten a closing tag");
    assert!(svg.contains("viewBox=\"0 0 "), "{name}");
    let width = dimension(svg, "width");
    let height = dimension(svg, "height");
    assert!(width > 0.0 && height > 0.0, "{name} {width}x{height}");
    let mut ids = std::collections::BTreeSet::new();
    let mut rest = svg;
    while let Some(index) = rest.find("id=\"") {
        rest = &rest[index + 4..];
        let id = rest.split('"').next().unwrap();
        assert!(ids.insert(id.to_string()), "{name} duplicate id {id}");
    }
    rest = svg;
    while let Some(index) = rest.find("url(#") {
        rest = &rest[index + 5..];
        let id = rest.split(['"', '\'', ')']).next().unwrap();
        assert!(ids.contains(id), "{name} unresolved #{id}");
    }
}

#[test]
fn windows_line_endings_render_the_same_picture() {
    let fixtures = root().join("tests/fixtures");
    let mut sources: Vec<PathBuf> = [fixtures.clone(), fixtures.join("legacy")]
        .iter()
        .flat_map(|dir| fs::read_dir(dir).unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext != "svg"))
        .collect();
    sources.sort();
    assert!(sources.len() > 8, "{sources:?}");
    for path in sources {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            !source.contains('\r'),
            "{} is checked out with CRLF",
            path.display()
        );
        let windows = source.replace('\n', "\r\n");
        match (tgw::render(&source), tgw::render(&windows)) {
            (Ok(unix), Ok(crlf)) => assert_eq!(unix, crlf, "{}", path.display()),
            (Err(_), Err(_)) => {}
            (unix, crlf) => panic!("{}: {unix:?} vs {crlf:?}", path.display()),
        }
    }
}

#[test]
fn long_clock_is_constant_paint() {
    let short = "{signal:[{name:'clk',wave:'p...'}]}";
    let mut dots = String::from("p");
    dots.extend(std::iter::repeat_n('.', 9_999));
    let long = format!("{{signal:[{{name:'clk',wave:'{dots}'}}]}}");
    let a = tgw::render(short).unwrap();
    let b = tgw::render(&long).unwrap();
    assert!(b.len() < 8_000, "svg bytes {}", b.len());
    assert!(!b.contains("<use"));
    assert!(b.contains("-k0\""));
    assert!(b.contains("H400000"));
    assert_eq!(tags(&a, "pattern"), tags(&b, "pattern"));
    assert_eq!(tags(&a, "rect"), tags(&b, "rect"));
    assert_eq!(tags(&a, "path"), tags(&b, "path"));
    assert_eq!(tags(&a, "use"), 0);
}

#[test]
fn long_hold_is_one_stroke() {
    let mut dots = String::from("1");
    dots.extend(std::iter::repeat_n('.', 9_999));
    let src = format!("{{signal:[{{name:'held',wave:'{dots}'}}]}}");
    let svg = tgw::render(&src).unwrap();
    assert!(svg.contains("H400000"));
    assert!(!svg.contains("<use"));
    let short = tgw::render("{signal:[{name:'held',wave:'1'}]}").unwrap();
    assert_eq!(tags(&short, "path"), tags(&svg, "path"));
    assert_eq!(svg.matches("H400000").count(), 1);
}

#[test]
fn clock_after_prefix_is_translated() {
    let svg = tgw::render("{signal:[{name:'c',wave:'xp'}]}").unwrap();
    assert!(svg.contains("translate(40)"));
    assert!(svg.contains("-k0)"));
}

#[test]
fn indent_and_errors() {
    let mut buf = Vec::new();
    tgw::render_opts("{signal:[{name:'a',wave:'01'}]}", &mut buf, 2).unwrap();
    let pretty = String::from_utf8(buf).unwrap();
    assert!(pretty.contains('\n'));
    let err = tgw::render("[").unwrap_err();
    assert_eq!(err.offset, 0);
    let err = tgw::render("{config:{hscale:1}}").unwrap_err();
    assert!(err.message.contains("signal"));
}

#[test]
fn buffer_is_reused() {
    let src = "{signal:[{name:'a',wave:'p...'}]}";
    let mut buf = Vec::new();
    tgw::render_into(src, &mut buf).unwrap();
    let first = buf.clone();
    buf.extend_from_slice(b"junk");
    tgw::render_into(src, &mut buf).unwrap();
    assert_eq!(buf, first);
}

#[test]
fn piecewise_and_comments() {
    let src = r#"{
      // a comment
      signal: [
        { name: 'pw', wave: ['pw', {d:'M0,0 L1,1'}], },
      ],
    }"#;
    let svg = tgw::render(src).unwrap();
    assert!(svg.contains("<path"));
    assert!(svg.contains("M0,0"));
}

fn dimension(svg: &str, attribute: &str) -> f64 {
    svg.split_once(&format!("{attribute}=\""))
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn fractional_and_negative_phase_have_correct_canvas_extents() {
    let plain = tgw::render("{signal:[{name:'clk',wave:'p...'}]}").unwrap();
    let delayed = tgw::render("{signal:[{name:'clk',wave:'p...',phase:-0.25}]}").unwrap();
    let advanced = tgw::render("{signal:[{name:'clk',wave:'p...',phase:0.125}]}").unwrap();
    assert_eq!(
        dimension(&delayed, "width") - dimension(&plain, "width"),
        10.0
    );
    assert_eq!(
        dimension(&plain, "width") - dimension(&advanced, "width"),
        5.0
    );
    assert!(advanced.contains("translate(-5)"));
    assert!(advanced.contains("-lane-crop)"));
}

#[test]
fn cropped_dense_waves_only_paint_the_visible_window() {
    let wave = "23456789".repeat(1250);
    let svg = tgw::render(&format!(
        "{{signal:[{{name:'data',wave:'{wave}'}}],config:{{hbounds:[5000,5005]}}}}"
    ))
    .unwrap();
    assert!(
        svg.len() < 12_000,
        "cropped SVG contains {} bytes",
        svg.len()
    );
    assert!(tags(&svg, "path") < 40);
}

#[test]
fn inline_documents_isolate_paint_and_preserve_text() {
    let a = tgw::render("{signal:[{name:'url(#k0)',wave:'p...'}]}").unwrap();
    let b = tgw::render("{signal:[{name:'second',wave:'n...'}]}").unwrap();
    let ids = |s: &str| -> Vec<String> {
        s.split(" id=\"")
            .skip(1)
            .map(|part| part.split('"').next().unwrap().to_string())
            .collect()
    };
    let a_ids = ids(&a);
    let b_ids = ids(&b);
    assert!(a_ids.iter().all(|id| !b_ids.contains(id)));
    for svg in [&a, &b] {
        let definitions = ids(svg);
        for reference in svg.split("url(#").skip(1) {
            let id = reference.split(')').next().unwrap();
            if id.starts_with("tgw-") {
                assert!(definitions.contains(&id.to_string()), "{id}");
            }
        }
        assert!(!svg.contains("<style"));
        assert!(svg.contains("role=\"img\""));
    }
    assert!(a.contains(">url(#k0)<"));
}

#[test]
fn paths_preserve_curves_arcs_and_native_scaling() {
    let svg = tgw::render("{signal:[{name:'analog',period:2,wave:['pw',{d:'M0,0 C1,0 1,1 2,1 A1,1 30 0 1 3,0'}]}],config:{hscale:2}}").unwrap();
    assert!(svg.contains("scale(160,-20)"));
    assert!(svg.contains("stroke-width=\"0.006\""));
    assert!(!svg.contains("vector-effect"));
    assert!(svg.contains("A1,1 30 0 1 3,0"));
    assert!(dimension(&svg, "width") > 480.0);
}

#[test]
fn errors_clear_reused_output_and_do_not_panic() {
    let mut out = vec![42; 1024];
    for source in [
        "{signal:[{wave:'p',period:-1}]}",
        "{signal:[{wave:['pw',{d:'M0 0 !'}]}]}",
        "{signal:[{wave:'p'}],head:{tick:'0 1e308'}}",
        "{signal:[] /*",
    ] {
        assert!(tgw::render_into(source, &mut out).is_err(), "{source}");
        assert!(out.is_empty());
        out.extend_from_slice(b"reuse");
    }
}

#[test]
fn malformed_utf8_source_fragments_are_bounded() {
    let alphabet = [
        '{', '}', '[', ']', '\"', '\\', '/', '*', ':', ',', '.', '+', '-', '0', '1', 'e', 'a', ' ',
        '\n', '波',
    ];
    let mut seed = 0x317f9_u64;
    for len in 0..160 {
        for _ in 0..12 {
            let mut source = String::from("{signal:");
            for _ in 0..len {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                source.push(alphabet[(seed >> 32) as usize % alphabet.len()]);
            }
            let _ = tgw::render(&source);
        }
    }
}
