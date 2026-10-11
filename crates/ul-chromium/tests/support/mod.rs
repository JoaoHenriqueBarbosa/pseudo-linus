//! O que os testes de perfil do `--dump-dom` repartem.

use sysabi::sys;
use sysabi::*;

/// Responde toda requisição com a página, em HTTP/1.0 como o `http.server` do Python.
pub fn serve(listener: Fd, page: &'static str) -> i32 {
    let s = sys::current();
    loop {
        let Ok((fd, _)) = s.tcp_accept(listener, false, true) else { continue };
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            match sys::read(fd, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&buffer[..n]),
            }
        }
        let response = format!("HTTP/1.0 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{page}", page.len());
        let _ = sys::write_all(fd, response.as_bytes());
        let _ = sys::close(fd);
    }
}
