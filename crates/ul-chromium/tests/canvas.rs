//! O canvas 2D contra o oráculo: as medidas do `measureText` em `canvas-measure.json` foram tiradas do Chromium do
//! Debian (largura, caixa de tinta e caixa da fonte, nessa ordem, para cada fonte e palavra).

mod common;

use common::probe_output;
use ul_chromium::canvas_text::{measure, CanvasFont};

const FIELDS: [&str; 7] = ["width", "actualBoundingBoxLeft", "actualBoundingBoxRight", "actualBoundingBoxAscent", "actualBoundingBoxDescent", "fontBoundingBoxAscent", "fontBoundingBoxDescent"];

#[test]
fn measure_text_matches_oracle() {
    common::install_image_fonts();
    let table: serde_json::Value = serde_json::from_str(include_str!("canvas-measure.json")).unwrap();
    let mut wrong = Vec::new();
    for entry in table.as_array().unwrap() {
        let font = CanvasFont {
            family: entry["family"].as_str().unwrap(),
            weight: entry["weight"].as_f64().unwrap() as f32,
            style: entry["style"].as_str().unwrap(),
            size: entry["size"].as_f64().unwrap() as f32,
        };
        for row in entry["rows"].as_array().unwrap() {
            let text = row[0].as_str().unwrap();
            let got = measure(text, &font, "alphabetic");
            for (i, field) in FIELDS.iter().enumerate() {
                let want = row[i + 1].as_f64().unwrap();
                if got[i] != want {
                    wrong.push(format!("{} {text:?} {field}: {} (oráculo {want})", entry["font"], got[i]));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{} divergências:\n{}", wrong.len(), wrong.join("\n"));
}

/// `isPointInPath` e `isPointInStroke` contra `testbench/oracle/measured/canvas-hit.html` (borda, regras, transformação,
/// cap, join e tracejado).
#[test]
fn hit_tests_match_oracle() {
    assert_eq!(probe_output(include_str!("../../../testbench/oracle/measured/canvas-hit.html")), serde_json::from_str::<serde_json::Value>(include_str!("canvas-hit.json")).unwrap());
}


/// O `toDataURL` contra `testbench/oracle/measured/canvas-encode.html`: PNG e JPEG byte a byte iguais aos do Chromium
/// nas cenas sem antisserrilhado.
#[test]
fn data_url_matches_oracle() {
    let ours = probe_output(include_str!("../../../testbench/oracle/measured/canvas-encode.html"));
    let oracle: serde_json::Value = serde_json::from_str(include_str!("canvas-encode.json")).unwrap();
    for (key, want) in oracle.as_object().unwrap() {
        assert_eq!(&ours[key], want, "{key}");
    }
}

/// As linhas de base do `textBaseline` pelo caminho inteiro (prelúdio e ponte), contra
/// `testbench/oracle/measured/canvas-baselines.html`: a página roda aqui e o resultado tem de ser o do oráculo.
#[test]
fn text_baselines_match_oracle() {
    let ours = probe_output(include_str!("../../../testbench/oracle/measured/canvas-baselines.html"));
    let oracle: serde_json::Value = serde_json::from_str(include_str!("canvas-baselines.json")).unwrap();
    assert_eq!(ours, oracle);
}

/// A volta da premultiplicação contra `testbench/oracle/measured/canvas-premul.html`: `getImageData` depois de
/// `putImageData` com cor e alfa aleatórios devolve os mesmos 899 pixels que o Chromium.
#[test]
fn premultiply_round_trip_matches_oracle() {
    assert_eq!(probe_output(include_str!("../../../testbench/oracle/measured/canvas-premul.html")), serde_json::from_str::<serde_json::Value>(include_str!("canvas-premul.json")).unwrap());
}

/// Os gradientes contra `testbench/oracle/measured/canvas-gradient.html` (linear com pontos quebrados, várias paradas,
/// transformação, `globalAlpha`, cônica e radial concêntrica): pixel a pixel iguais aos do Chromium, dither incluído.
#[test]
fn gradients_match_oracle() {
    let ours = probe_output(include_str!("../../../testbench/oracle/measured/canvas-gradient.html"));
    let oracle: serde_json::Value = serde_json::from_str(include_str!("canvas-gradient.json")).unwrap();
    for (key, want) in oracle.as_object().unwrap() {
        let (got, want) = (ours[key].as_array().unwrap(), want.as_array().unwrap());
        let wrong = got.iter().zip(want).filter(|(a, b)| a != b).count();
        assert_eq!(wrong, 0, "{key}: {wrong} de {} canais diferentes", want.len());
    }
}

/// O `toDataURL('image/webp')` contra o oráculo, byte a byte: `canvas-webp.html` (cenas desenhadas), `canvas-webp-scenes.html`
/// (pixels postos com putImageData: com perdas, alfa, sem perdas, WebGL) e `canvas-webp-icc.html` (quando entra o
/// perfil sRGB).
#[test]
fn webp_data_url_matches_oracle() {
    let probes = [
        (include_str!("../../../testbench/oracle/measured/canvas-webp.html"), include_str!("canvas-webp.json")),
        (include_str!("../../../testbench/oracle/measured/canvas-webp-scenes.html"), include_str!("canvas-webp-scenes.json")),
        (include_str!("../../../testbench/oracle/measured/canvas-webp-icc.html"), include_str!("canvas-webp-icc.json")),
    ];
    let mut wrong = Vec::new();
    for (probe, measured) in probes {
        let ours = probe_output(probe);
        let oracle: serde_json::Value = serde_json::from_str(measured).unwrap();
        wrong.extend(oracle.as_object().unwrap().iter().filter(|(key, want)| &ours[key.as_str()] != *want).map(|(key, _)| key.clone()));
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// A rasterização contra o oráculo: as cenas de `testbench/oracle/measured/canvas-raster.html` rodam aqui e os pixels
/// se comparam com os do Chromium (`canvas-raster.json`). O antisserrilhado do vello não é o do Skia bit a bit, então
/// a régua é a de quem olha a imagem: diferença média por canal até 1,5 e no máximo 2% dos canais fora de 40.
#[test]
fn raster_matches_oracle() {
    let scenes = include_str!("../../../testbench/oracle/measured/canvas-raster.html");
    let oracle = include_str!("canvas-raster.json");
    let compare = format!(
        "<script>const oracle = {oracle};
const ours = JSON.parse(document.body.getAttribute('data-out'));
for (const [name, expected] of Object.entries(oracle)) {{
  const a = atob(expected), b = atob(ours[name] || '');
  let sum = 0, bad = 0;
  for (let i = 0; i < a.length; i++) {{ const d = Math.abs(a.charCodeAt(i) - (b.charCodeAt(i) || 0)); sum += d; if (d > 40) bad++; }}
  console.log(name + ' ' + (sum / a.length).toFixed(3) + ' ' + (bad / a.length).toFixed(4));
  if (sum / a.length > 1.5 || bad / a.length > 0.02) {{
    const px = (s, i) => [0, 1, 2, 3].map((k) => s.charCodeAt(i + k) || 0).join(',');
    const rows = [];
    for (let i = 0; i < a.length && rows.length < 12; i += 4) if (px(a, i) !== px(b, i)) rows.push((i / 4 % 32) + ':' + Math.floor(i / 128) + ' ' + px(a, i) + ' / ' + px(b, i));
    console.error('  ' + rows.join('  '));
  }}
}}</script>"
    );
    let page = scenes.replace("</body>", &format!("{compare}</body>"));
    let (_, log) = common::run(&[("/", &page)]);
    let scores: Vec<&str> = log.lines().filter(|l| !l.starts_with(' ')).collect();
    let mut wrong = Vec::new();
    for line in &scores {
        let parts: Vec<&str> = line.split(' ').collect();
        let (mean, bad): (f64, f64) = (parts[1].parse().unwrap(), parts[2].parse().unwrap());
        if mean > 1.5 || bad > 0.02 {
            wrong.push((*line).to_owned());
        }
    }
    eprintln!("{log}");
    assert!(scores.len() == 27 && wrong.is_empty(), "cenas fora da régua (nome, diferença média, fração ruim):\n{}\n\nlog:\n{log}", wrong.join("\n"));
}

/// A rasterização do WebGL contra o SwiftShader do oráculo (`testbench/oracle/measured/webgl-raster.html`): a cobertura
/// (quais pixels e quantas amostras) é exata, e cada canal fica a no máximo 1 do oráculo. O 1 que sobra é a última
/// casa do binário32: a mistura do SwiftShader para alvos de 8 bits e o `mulAdd` da interpolação ainda não são portes
/// bit a bit (as cenas sem mistura e com atributo variável já saem iguais).
#[test]
fn webgl_raster_matches_oracle() {
    let ours = probe_output(include_str!("../../../testbench/oracle/measured/webgl-raster.html"));
    let oracle: serde_json::Value = serde_json::from_str(include_str!("webgl-raster.json")).unwrap();
    let mut wrong = Vec::new();
    for (name, want) in oracle.as_object().unwrap() {
        // O toDataURL das cenas sem mistura sai byte a byte igual; o da mistura herda a casa do binário32 de cima.
        if let Some(scene) = name.strip_suffix(":png") {
            if scene != "aa-blend" {
                assert_eq!(&ours[name], want, "{name}");
            }
            continue;
        }
        let decode = |v: &serde_json::Value| ul_common::codec::base64_decode(v.as_str().unwrap_or("").as_bytes(), true).unwrap_or_default();
        let (a, b) = (decode(want), decode(&ours[name]));
        assert_eq!(a.len(), b.len(), "{name}");
        let off = |i: usize| (a[i] as i32 - b[i] as i32).abs();
        let coverage = |p: &[u8], i: usize| p[i..i + 4] == [0, 0, 0, 0];
        let diff: Vec<usize> = (0..a.len()).step_by(4).filter(|&i| coverage(&a, i) != coverage(&b, i) || (i..i + 4).any(|k| off(k) > 1)).collect();
        if !diff.is_empty() {
            let shown: Vec<String> = diff.iter().take(6).map(|&i| format!("({},{}) {:?} / {:?}", i / 4 % 24, i / 96, &a[i..i + 4], &b[i..i + 4])).collect();
            wrong.push(format!("{name}: {} pixels fora; {}", diff.len(), shown.join(" ")));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// As texturas do WebGL contra o SwiftShader do oráculo (`testbench/oracle/measured/webgl-texture.html`): envio de
/// typed array, ImageData, canvas e imagem com os formatos e as opções de desempacotamento, e a amostragem (filtros,
/// repetição, mipmaps, cubo, 3D, arrays, comparação), pixel a pixel.
#[test]
fn webgl_textures_match_oracle() {
    assert_scenes_exact(include_str!("../../../testbench/oracle/measured/webgl-texture.html"), include_str!("webgl-texture.json"));
}

/// O estêncil (operações, funções, máscaras, faces separadas, zfail, referência fora da faixa, clear com máscara) e o
/// polygon offset (unidades em três profundidades, fator com declive em x e em y) contra
/// `testbench/oracle/measured/webgl-stencil.html`, pixel a pixel.
#[test]
fn webgl_stencil_and_offset_match_oracle() {
    assert_scenes_exact(include_str!("../../../testbench/oracle/measured/webgl-stencil.html"), include_str!("webgl-stencil.json"));
}

/// Pontos (tamanhos fracionários, recorte só por w e z, gl_PointCoord) e linhas Bresenham (eixos, diagonais, faixa,
/// laço, recorte, profundidade, interpolação pela projeção no segmento) contra `testbench/oracle/measured/webgl-prims.html`,
/// pixel a pixel.
#[test]
fn webgl_points_and_lines_match_oracle() {
    assert_scenes_exact(include_str!("../../../testbench/oracle/measured/webgl-prims.html"), include_str!("webgl-prims.json"));
}

/// O render em framebuffer (textura RGBA8 e renderbuffers RGBA4 e DEPTH_STENCIL, sem a inversão do buffer padrão:
/// arestas, gl_FragCoord, faces, derivadas, pontos, linhas, viewport, tesoura, clear e mistura) e a textura
/// renderizada amostrada de volta, contra `testbench/oracle/measured/webgl-fbo.html`, pixel a pixel.
#[test]
fn webgl_framebuffers_match_oracle() {
    assert_scenes_exact(include_str!("../../../testbench/oracle/measured/webgl-fbo.html"), include_str!("webgl-fbo.json"));
}

/// Anexo de cor em ponto flutuante (lido em binário32: as unidades de 4 bytes são canais), DEPTH_COMPONENT16 e o
/// erro de feedback loop, iguais ao oráculo.
#[test]
fn webgl_framebuffer_gaps_match_oracle() {
    assert_scenes_exact(include_str!("../../../testbench/oracle/measured/webgl-fbo-gaps.html"), include_str!("webgl-fbo-gaps.json"));
}

/// Roda a sonda e exige cada cena (readPixels em base64) igual à do oráculo em todos os pixels.
fn assert_scenes_exact(probe: &str, oracle: &str) {
    let ours = probe_output(probe);
    let oracle: serde_json::Value = serde_json::from_str(oracle).unwrap();
    let decode = |v: &serde_json::Value| ul_common::codec::base64_decode(v.as_str().unwrap_or("").as_bytes(), true).unwrap_or_default();
    let wrong: Vec<String> = oracle
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(name, want)| {
            // Valor que não é base64 (status, getError, extensão presente) se compara direto.
            if !want.is_string() {
                return (*want != ours[name]).then(|| format!("{name}: {want} no oráculo, {} aqui", ours[name]));
            }
            let (a, b) = (decode(want), decode(&ours[name]));
            if a.len() != b.len() {
                return Some(format!("{name}: {} bytes no oráculo, {} aqui", a.len(), b.len()));
            }
            let bad: Vec<usize> = (0..a.len()).step_by(4).filter(|&i| a[i..i + 4] != b[i..i + 4]).collect();
            // Os primeiros pixels divergentes: índice do pixel (de baixo para cima), oráculo / nosso.
            let shown: Vec<String> = bad.iter().take(8).map(|&i| format!("#{} {:?} / {:?}", i / 4, &a[i..i + 4], &b[i..i + 4])).collect();
            (!bad.is_empty()).then(|| format!("{name}: {} pixels; {}", bad.len(), shown.join(" ")))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
