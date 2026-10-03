//! A política de rede é nossa: estes testes falham se ela deixar passar o que não deve.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;

use f12_net_sqlite_git::net::experiments::Server;
use f12_net_sqlite_git::net::{Policy, sandbox_agent};

fn table(pairs: &[(&str, &str)]) -> BTreeMap<String, Vec<IpAddr>> {
    pairs.iter().map(|(h, ip)| (h.to_string(), vec![ip.parse().unwrap()])).collect()
}

#[test]
fn allowed_passes_and_denied_never_connects() {
    let s = Server::start("127.0.0.1", None).unwrap();
    let policy = Arc::new(Policy::new(&["allowed.test"], false));
    let (agent, lookups) = sandbox_agent(policy, table(&[("allowed.test", "127.0.0.1"), ("denied.test", "127.0.0.1")]));
    let mut ok = agent.get(&format!("http://allowed.test:{}/ok", s.port())).call().unwrap();
    assert_eq!(ok.status().as_u16(), 200);
    assert!(ok.body_mut().read_to_string().unwrap().starts_with("hello"));
    assert!(agent.get(&format!("http://denied.test:{}/ok", s.port())).call().is_err());
    assert!(agent.get(&format!("http://127.0.0.1:{}/ok", s.port())).call().is_err());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(s.connections(), 1, "só a requisição permitida pode chegar no servidor");
    assert_eq!(lookups.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[test]
fn redirect_to_denied_host_is_blocked() {
    let target = Server::start("127.0.0.2", None).unwrap();
    let front = Server::start("127.0.0.1", Some(format!("http://denied.test:{}/ok", target.port()))).unwrap();
    let policy = Arc::new(Policy::new(&["allowed.test"], false));
    let (agent, _) = sandbox_agent(policy, table(&[("allowed.test", "127.0.0.1"), ("denied.test", "127.0.0.2")]));
    assert!(agent.get(&format!("http://allowed.test:{}/redirect", front.port())).call().is_err());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(front.connections(), 1);
    assert_eq!(target.connections(), 0);
}

#[test]
fn private_resolution_blocked_when_policy_says() {
    let s = Server::start("127.0.0.1", None).unwrap();
    let policy = Arc::new(Policy::new(&["rebind.test"], true));
    let (agent, _) = sandbox_agent(policy, table(&[("rebind.test", "127.0.0.1")]));
    assert!(agent.get(&format!("http://rebind.test:{}/ok", s.port())).call().is_err());
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(s.connections(), 0);
}
