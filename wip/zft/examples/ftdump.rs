//! Mesmo formato de saída do `ftref.c` de referência, para comparar com `diff`.
//! uso: ftdump fonte.ttf tamanho flags texto

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&a[1]).expect("fonte");
    let size: f32 = a[2].parse().unwrap();
    let f: u32 = a[3].parse().unwrap();
    let mut face = zft::Face::new(data, 0).expect("face");
    let w = (size * 64.0) as i64;
    face.request_size(w, w).expect("tamanho");
    let m = face.size;
    println!(
        "size x_ppem={} y_ppem={} xs={} ys={} asc={} desc={} h={} maxadv={}",
        m.x_ppem, m.y_ppem, m.x_scale, m.y_scale, m.ascender, m.descender, m.height, m.max_advance
    );
    let mut load = 0;
    if f & 1 != 0 {
        load |= zft::LOAD_NO_HINTING;
    }
    if f & 2 != 0 {
        load |= zft::LOAD_TARGET_MONO;
    }
    if f & 4 != 0 {
        load |= zft::LOAD_FORCE_AUTOHINT;
    }
    for ch in a[4].chars() {
        let gi = face.char_index(ch as u32);
        let g = match face.load_glyph(gi, load) {
            Ok(g) => g,
            Err(_) => {
                println!("erro {}", ch as u32);
                continue;
            }
        };
        println!(
            "glyph U+{:04X} gi={} adv={} lsb_delta={} rsb_delta={} w={} h={} bx={} by={}",
            ch as u32, gi, g.hori_advance, g.lsb_delta, g.rsb_delta, g.width, g.height, g.hori_bearing_x, g.hori_bearing_y
        );
        let o = &g.outline;
        print!("outline n={} c={}:", o.points.len(), o.contours.len());
        for c in &o.contours {
            print!(" {c}");
        }
        println!();
        for (p, t) in o.points.iter().zip(&o.tags) {
            print!(" {},{},{}", p.x, p.y, t & 3);
        }
        println!();
        let b = o.cbox();
        println!("cbox {} {} {} {}", b.x_min >> 6, b.y_min >> 6, (b.x_max + 63) >> 6, (b.y_max + 63) >> 6);
        let bm = g.render().expect("render");
        println!("bitmap left={} top={} rows={} width={} pitch={} mode=2", bm.left, bm.top, bm.rows, bm.width, bm.pitch);
        for y in 0..bm.rows as usize {
            for x in 0..bm.pitch as usize {
                print!("{:02x}", bm.buffer[y * bm.pitch as usize + x]);
            }
            println!();
        }
    }
}
