use std::hint::black_box;
use std::time::{Duration, Instant};

fn main() {
    let clock = format!("p{}", ".".repeat(9_999));
    let clock_src = format!("{{signal:[{{name:'clk',wave:'{clock}'}}]}}");
    let held = format!("1{}", ".".repeat(9_999));
    let held_src = format!("{{signal:[{{name:'held',wave:'{held}'}}]}}");

    let mut bus = String::with_capacity(10_000);
    for i in 0..10_000 {
        bus.push(char::from(b'2' + (i % 8) as u8));
    }
    let bus_src = format!("{{signal:[{{name:'bus',wave:'{}'}}]}}", &bus[..1_000]);
    let cropped_src =
        format!("{{signal:[{{name:'bus',wave:'{bus}'}}],config:{{hbounds:[4990,5010]}}}}");

    println!("Repeated release-mode renders; warmed caller-owned output buffer.");
    measure("clock 10,000 cycles [JSON5]", &clock_src);
    measure("clock 10,000 cycles [native]", &format!("clk: {clock}"));
    measure("held 10,000 cycles [JSON5]", &held_src);
    measure("held 10,000 cycles [native]", &format!("held: {held}"));
    measure("bus 1,000 values [JSON5]", &bus_src);
    measure(
        "bus 1,000 values [native]",
        &format!("bus: {}", &bus[..1_000]),
    );
    measure("bus 10,000 values, 20 visible [JSON5]", &cropped_src);
    measure(
        "bus 10,000 values, 20 visible [native]",
        &format!("@bounds 4990 5010\nbus: {bus}"),
    );
}

fn measure(name: &str, source: &str) {
    let mut buf = Vec::new();
    for _ in 0..10 {
        tgw::render_into(black_box(source), &mut buf).unwrap();
        black_box(&buf);
    }

    // Measure several samples rather than comparing two noisy single runs.
    let mut samples = Vec::with_capacity(200);
    let started = Instant::now();
    while samples.len() < 200 && (samples.len() < 30 || started.elapsed() < Duration::from_secs(1))
    {
        let t = Instant::now();
        tgw::render_into(black_box(source), &mut buf).unwrap();
        samples.push(t.elapsed());
        black_box(&buf);
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[(samples.len() - 1) * 95 / 100];
    println!(
        "{name}: {} bytes, median {:.1} us, p95 {:.1} us ({} samples)",
        buf.len(),
        median.as_secs_f64() * 1e6,
        p95.as_secs_f64() * 1e6,
        samples.len(),
    );
}
