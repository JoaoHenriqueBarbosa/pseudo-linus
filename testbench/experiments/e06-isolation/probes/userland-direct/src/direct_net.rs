//! `std::net::TcpStream::connect` chamado direto.

pub fn can_connect(addr: &str) -> bool {
    std::net::TcpStream::connect(addr).is_ok()
}
