use std::time::Instant;

fn main() {
    let mut clock = String::from("p");
    clock.extend(std::iter::repeat('.').take(9_999));
    let clock_src = format!("{{signal:[{{name:'clk',wave:'{clock}'}}]}}");

    let mut bus = String::new();
    for i in 0..200 {
        bus.push(char::from(b'2' + (i % 8) as u8));
    }
    let bus_src = format!("{{signal:[{{name:'bus',wave:'{bus}',data:'d'}}]}}");

    let mut buf = Vec::new();
    let t = Instant::now();
    tgw::render_into(&clock_src, &mut buf).unwrap();
    let first = t.elapsed();
    let clock_bytes = buf.len();
    let t = Instant::now();
    tgw::render_into(&clock_src, &mut buf).unwrap();
    let second = t.elapsed();
    println!(
        "clock 10000 cycles: {} bytes, first {} us, reuse {} us",
        clock_bytes,
        first.as_micros(),
        second.as_micros()
    );

    let t = Instant::now();
    tgw::render_into(&bus_src, &mut buf).unwrap();
    println!(
        "bus 200 values: {} bytes, {} us",
        buf.len(),
        t.elapsed().as_micros()
    );
}
