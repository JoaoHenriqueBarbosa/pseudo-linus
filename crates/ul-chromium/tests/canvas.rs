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
            let got = measure(text, &font);
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
