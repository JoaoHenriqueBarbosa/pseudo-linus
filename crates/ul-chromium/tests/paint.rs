//! A pintura da página contra o screenshot do Chromium 154 do oráculo (`testbench/oracle`, mesmo viewport de 780x493,
//! `--hide-scrollbars`, as fontes do Debian). A conformidade é perceptual, não byte a byte: ver `ssim` abaixo.
//!
//! Se existir `tests/paint/out/`, o PNG que a pintura produziu vai para lá (para olhar ao lado do golden).

mod common;

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use kernel::{Kernel, KernelConfig, RunRequest, SandboxConfig};
use sysabi::sys;
use sysabi::*;
use ul_chromium::page::Page;

const WIDTH: u32 = 780;
const HEIGHT: u32 = 493;

/// A métrica `paint-ssim-v1`: SSIM médio (Wang et al., 2004) na luminância Rec. 709, janela uniforme de 8x8 com passo 4,
/// constantes padrão (L=255, K1=0,01, K2=0,03). Escolhida porque pesa estrutura (bordas, glifos, alinhamento) e não o
/// valor absoluto de cada pixel: o antisserrilhado e o hinting do texto diferem do Skia sem que a página pareça outra,
/// e o SSIM cai quando um elemento muda de lugar, de tamanho ou some (o que o erro médio quadrático esconde numa
/// página quase toda branca).
fn ssim(a: &[u8], b: &[u8], width: usize, height: usize) -> f64 {
    let luma = |rgba: &[u8]| -> Vec<f64> { rgba.chunks_exact(4).map(|p| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])).collect() };
    let (a, b) = (luma(a), luma(b));
    let (c1, c2) = ((0.01_f64 * 255.0).powi(2), (0.03_f64 * 255.0).powi(2));
    let (mut sum, mut count) = (0.0, 0u32);
    let mut y = 0;
    while y + 8 <= height {
        let mut x = 0;
        while x + 8 <= width {
            let (mut ma, mut mb) = (0.0, 0.0);
            for j in 0..8 {
                for i in 0..8 {
                    ma += a[(y + j) * width + x + i];
                    mb += b[(y + j) * width + x + i];
                }
            }
            let (ma, mb) = (ma / 64.0, mb / 64.0);
            let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
            for j in 0..8 {
                for i in 0..8 {
                    let (da, db) = (a[(y + j) * width + x + i] - ma, b[(y + j) * width + x + i] - mb);
                    va += da * da;
                    vb += db * db;
                    cov += da * db;
                }
            }
            let (va, vb, cov) = (va / 63.0, vb / 63.0, cov / 63.0);
            sum += ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            count += 1;
            x += 4;
        }
        y += 4;
    }
    sum / f64::from(count)
}

fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).unwrap().to_rgba8();
    (image.width(), image.height(), image.into_raw())
}

fn painted(name: &str) -> Vec<u8> {
    common::install_image_fonts();
    let html = std::fs::read_to_string(format!("{}/tests/paint/pages/{name}.html", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut page = Page::load("http://t.test/", common::serve(&[("/", &html)], &[])).unwrap();
    page.run();
    let png = page.screenshot_png(WIDTH, HEIGHT);
    let dir = format!("{}/tests/paint/out", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&dir).is_dir() {
        std::fs::write(format!("{dir}/{name}.png"), &png).unwrap();
    }
    png
}

#[test]
fn paint_matches_the_oracle_screenshots() {
    let mut worst = 1.0_f64;
    // O piso de cada página: o medido menos 0,02 (catraca, só sobe).
    for (name, floor) in [("text", 0.85), ("box", 0.86), ("image", 0.87), ("flex_grid", 0.81), ("site", 0.72)] {
        let golden = std::fs::read(format!("{}/tests/paint/golden/{name}.png", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let (gw, gh, expected) = decode(&golden);
        let (w, h, actual) = decode(&painted(name));
        assert_eq!((gw, gh), (WIDTH, HEIGHT), "{name}: tamanho do golden");
        assert_eq!((w, h), (WIDTH, HEIGHT), "{name}: tamanho da pintura");
        let score = ssim(&expected, &actual, WIDTH as usize, HEIGHT as usize);
        eprintln!("paint {name}: ssim {score:.4}");
        assert!(score >= floor, "{name}: ssim {score:.4} abaixo do piso {floor}");
        worst = worst.min(score);
    }
    eprintln!("paint pior ssim {worst:.4}");
}

const TEXT_PAGE: &str = include_str!("paint/pages/text.html");

/// O `chromium --headless --screenshot --window-size` dentro do sandbox: escreve o PNG, avisa na saída de erro como o
/// Chromium e o arquivo é a mesma pintura (o PNG vai em hexadecimal para a saída do cliente).
fn screenshot_client(ctx: &mut Ctx, _argv: &[OsString]) -> i32 {
    let s = sys::current();
    sysabi::util::write_file(b"/tmp/a.html", TEXT_PAGE.as_bytes(), 0o644).unwrap();
    let argv: Vec<Vec<u8>> = ["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--hide-scrollbars", "--screenshot=/tmp/s.png", "--window-size=780,493", "file:///tmp/a.html"].iter().map(|a| a.as_bytes().to_vec()).collect();
    let pid = s.spawn(SpawnSpec { path: b"/usr/bin/chromium".to_vec(), argv, attrs: ProcAttrs::default() }).unwrap();
    let status = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap().unwrap().1;
    let png = sys::read_file(b"/tmp/s.png").unwrap_or_default();
    let hex: String = png.iter().map(|byte| format!("{byte:02x}")).collect();
    let _ = ctx.stdout().write_all(format!("{status:?}\n{hex}\n").as_bytes());
    0
}

#[test]
fn the_screenshot_flag_writes_the_painted_page() {
    let kernel = Kernel::new(KernelConfig::default());
    let mut programs = ul_chromium::programs();
    programs.push(Program::bin("shot_client", screenshot_client));
    let sandbox = kernel.create_sandbox(SandboxConfig { programs, ..SandboxConfig::default() }).unwrap();
    let output = sandbox.run(RunRequest { argv: vec![b"shot_client".to_vec()], timeout: Some(Duration::from_secs(120)), ..RunRequest::default() }).unwrap();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut lines = text.lines();
    let status = lines.next().unwrap_or_default();
    assert!(status.contains("Exited(0)"), "status {status}; stderr {}", String::from_utf8_lossy(&output.stderr));
    let png: Vec<u8> = lines.next().unwrap_or_default().as_bytes().chunks(2).map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap()).collect();
    let (w, h, actual) = decode(&png);
    assert_eq!((w, h), (WIDTH, HEIGHT));
    let golden = std::fs::read(format!("{}/tests/paint/golden/text.png", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let (_, _, expected) = decode(&golden);
    let score = ssim(&expected, &actual, WIDTH as usize, HEIGHT as usize);
    eprintln!("paint cli text: ssim {score:.4}");
    assert!(score >= 0.85, "ssim {score}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("bytes written to file /tmp/s.png"), "stderr {}", String::from_utf8_lossy(&output.stderr));
}
