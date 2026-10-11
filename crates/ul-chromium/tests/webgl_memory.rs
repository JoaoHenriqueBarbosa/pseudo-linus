//! Memória das texturas do WebGL. Num binário à parte para o pico de memória residente do processo ser só deste
//! teste.

mod common;

/// O pico de memória residente do processo, em KiB (`VmHWM` de `/proc/self/status`).
fn peak_rss_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status.lines().find_map(|l| l.strip_prefix("VmHWM:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok()).unwrap()
}

/// Envios de 512x512 (ImageData e typed array, com UNPACK_FLIP_Y e UNPACK_PREMULTIPLY_ALPHA), o mipmap e texSubImage2D
/// intercalados com desenhos que a página não lê. A imagem de 852x492 que o Rive da vite.dev envia passava de 60 GB
/// com um array por texel (o zjsc ainda não coleta lixo: toda célula vive até o fim da página), e cada texSubImage
/// depois de um desenho pendente deixava na fila uma cópia inteira da textura. O canvas de 1x1 deixa a rasterização
/// de fora da medida, e a página vazia antes dela tira da conta o que o prelúdio ocupa.
#[test]
fn large_texture_uploads_stay_bounded() {
    let page = r#"<!doctype html><canvas id="c" width="1" height="1"></canvas><script>
const gl = document.getElementById('c').getContext('webgl', { antialias: false });
const N = 512, img = new ImageData(N, N);
for (let i = 0; i < img.data.length; i++) img.data[i] = i % 4 === 3 ? 255 : (i * 7) & 255;
gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, true);
gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
gl.generateMipmap(gl.TEXTURE_2D);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
const shader = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s); return s; };
const prog = gl.createProgram();
gl.attachShader(prog, shader(gl.VERTEX_SHADER, 'attribute vec2 p; varying vec2 v; void main() { v = p * 0.5 + 0.5; gl_Position = vec4(p, 0.0, 1.0); }'));
gl.attachShader(prog, shader(gl.FRAGMENT_SHADER, 'precision mediump float; uniform sampler2D s; varying vec2 v; void main() { gl_FragColor = texture2D(s, v); }'));
gl.linkProgram(prog);
gl.useProgram(prog);
gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
gl.enableVertexAttribArray(0);
gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
const sub = new Uint8Array(256 * 256 * 4);
for (let k = 0; k < 4; k++) {
  sub.fill(k * 8);
  gl.texSubImage2D(gl.TEXTURE_2D, 0, (k % 2) * 256, (k >> 1) * 256, 256, 256, gl.RGBA, gl.UNSIGNED_BYTE, sub);
  gl.drawArrays(gl.TRIANGLES, 0, 6);
}
const full = new Uint8Array(N * N * 4);
for (let i = 0; i < full.length; i += 4) { full[i] = 10; full[i + 1] = 20; full[i + 2] = 30; full[i + 3] = 255; }
gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, N, N, gl.RGBA, gl.UNSIGNED_BYTE, full);
gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
gl.drawArrays(gl.TRIANGLES, 0, 6);
const px = new Uint8Array(4);
gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
document.body.dataset.out = JSON.stringify(Array.from(px));
</script>"#;
    common::run(&[("/", "<!doctype html><canvas></canvas>")]);
    let base = peak_rss_kib();
    assert_eq!(common::probe_output(page), serde_json::json!([10, 20, 30, 255]));
    let grown = peak_rss_kib() - base;
    assert!(grown < BUDGET_KIB, "a página cresceu {grown} KiB, acima de {BUDGET_KIB} KiB");
}

/// O teto do crescimento. Plano, a página cresce uns 65 MiB (fontes, cópias, as imagens com o mipmap, o interpretador
/// sem coleta); com um array por texel, o envio de 852x492 da vite.dev passava de 8 GiB.
const BUDGET_KIB: u64 = 192 * 1024;
