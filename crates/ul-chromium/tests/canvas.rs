//! O canvas 2D contra o oráculo: as medidas do `measureText` em `canvas-measure.json` foram tiradas do Chromium do
//! Debian (largura, caixa de tinta e caixa da fonte, nessa ordem, para cada fonte e palavra).

mod common;

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

/// O `toDataURL` contra `testbench/oracle/measured/canvas-encode.html`: PNG e JPEG byte a byte iguais aos do Chromium
/// nas cenas sem antisserrilhado.
#[test]
fn data_url_matches_oracle() {
    let probe = include_str!("../../../testbench/oracle/measured/canvas-encode.html");
    let (dom, _) = common::run(&[("/", probe)]);
    let start = dom.find("data-out=\"").unwrap() + 10;
    let ours: serde_json::Value = serde_json::from_str(&dom[start..start + dom[start..].find('"').unwrap()].replace("&quot;", "\"")).unwrap();
    let oracle: serde_json::Value = serde_json::from_str(include_str!("canvas-encode.json")).unwrap();
    for (key, want) in oracle.as_object().unwrap() {
        assert_eq!(&ours[key], want, "{key}");
    }
}

/// As linhas de base do `textBaseline` pelo caminho inteiro (prelúdio e ponte), contra
/// `testbench/oracle/measured/canvas-baselines.html`: a página roda aqui e o resultado tem de ser o do oráculo.
#[test]
fn text_baselines_match_oracle() {
    let probe = include_str!("../../../testbench/oracle/measured/canvas-baselines.html");
    let (dom, _) = common::run(&[("/", probe)]);
    let start = dom.find("data-out=\"").unwrap() + 10;
    let ours: serde_json::Value = serde_json::from_str(&dom[start..start + dom[start..].find('"').unwrap()].replace("&quot;", "\"")).unwrap();
    let oracle: serde_json::Value = serde_json::from_str(include_str!("canvas-baselines.json")).unwrap();
    assert_eq!(ours, oracle);
}

/// A rasterização contra o oráculo: as cenas de `testbench/oracle/measured/canvas-raster.html` rodam aqui e os pixels
/// se comparam com os do Chromium (`canvas-raster.json`). O antisserrilhado do vello não é o do Skia bit a bit, então
/// a régua é a de quem olha a imagem: diferença média por canal até 1,5 e no máximo 2% dos canais fora de 40.
/// Exceção conhecida, com régua própria: o traço fino de cúbica (`bezier`), em que o stroker do Skia aproxima o
/// offset da curva por quadráticas e se afasta da curva exata que o kurbo expande (pendente: portar o `SkStroke`).
#[test]
fn raster_matches_oracle() {
    const KNOWN: &[(&str, f64, f64)] = &[("bezier", 1.7, 0.012)];
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
        let (max_mean, max_bad) = KNOWN.iter().find(|k| k.0 == parts[0]).map_or((1.5, 0.02), |k| (k.1, k.2));
        if mean > max_mean || bad > max_bad {
            wrong.push((*line).to_owned());
        }
    }
    eprintln!("{log}");
    assert!(scores.len() == 27 && wrong.is_empty(), "cenas fora da régua (nome, diferença média, fração ruim):\n{}\n\nlog:\n{log}", wrong.join("\n"));
}
