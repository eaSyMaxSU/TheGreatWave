use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture(name: &str) -> String {
    let path = root().join("tests/fixtures").join(format!("{name}.json5"));
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
    assert!(step.contains("url(#k"));
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
    assert!(clocks.contains("<pattern id=\"k"));

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
    assert!(marks.contains("WaveDrom example"));
    assert!(marks.contains("Figure 100"));
    assert!(marks.contains(">0<"));
    assert!(marks.contains(">9<"));
    assert!(marks.contains("id=\"gd\""));
}

#[test]
fn snapshots() {
    let mut wrote = Vec::new();
    for name in ["step4", "arcs", "arcs1", "clocks", "gaps", "bundles", "marks"] {
        let svg = render_fixture(name);
        let path = root().join("tests/fixtures").join(format!("{name}.svg"));
        match fs::read_to_string(&path) {
            Ok(expected) => assert_eq!(expected, svg, "snapshot {name}"),
            Err(_) => {
                fs::write(&path, &svg).unwrap();
                wrote.push(name);
            }
        }
    }
    assert!(wrote.is_empty(), "wrote snapshots {wrote:?}; re-run after review");
}

#[test]
fn long_clock_is_constant_paint() {
    let short = "{signal:[{name:'clk',wave:'p...'}]}";
    let mut dots = String::from("p");
    dots.extend(std::iter::repeat('.').take(9_999));
    let long = format!("{{signal:[{{name:'clk',wave:'{dots}'}}]}}");
    let a = tgw::render(short).unwrap();
    let b = tgw::render(&long).unwrap();
    assert!(b.len() < 8_000, "svg bytes {}", b.len());
    assert!(!b.contains("<use"));
    assert!(b.contains("<pattern id=\"k0\""));
    assert!(b.contains("width=\"400000\""));
    assert_eq!(tags(&a, "pattern"), tags(&b, "pattern"));
    assert_eq!(tags(&a, "rect"), tags(&b, "rect"));
    assert_eq!(tags(&a, "path"), tags(&b, "path"));
    assert_eq!(tags(&a, "use"), 0);
}

#[test]
fn long_hold_is_one_stroke() {
    let mut dots = String::from("1");
    dots.extend(std::iter::repeat('.').take(9_999));
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
    assert!(svg.contains("url(#k0)"));
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
