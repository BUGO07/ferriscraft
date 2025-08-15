use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, UdpSocket},
};

use renet_netcode::NetcodeServerTransport;

#[macro_export]
macro_rules! log {
    ($logs:expr, $($arg:tt)*) => ($crate::utils::_log($logs, format_args!($($arg)*)));
}

pub fn _log(logs: &mut VecDeque<String>, args: std::fmt::Arguments) {
    let s = chrono::Local::now().format("[%H:%M:%S] ").to_string() + &args.to_string();
    println!("{}", s);
    if logs.len() == 256 {
        logs.pop_front();
    }
    logs.push_back(s);
}

fn trimmed(b: &[u8]) -> &[u8] {
    &b[..b.iter().rposition(|&x| x != 0).map_or(0, |p| p + 1)]
}

pub fn get_name(client_id: u64, transport: &NetcodeServerTransport) -> Option<String> {
    Some(String::from_utf8_lossy(trimmed(&transport.user_data(client_id)?)).into_owned())
}

pub fn local_ip() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    sock.connect(("8.8.8.8", 80)).ok()?; // doesn't send any packets
    let addr = sock.local_addr().ok()?.ip().to_canonical();
    match addr {
        IpAddr::V4(ip) => {
            if is_private(ip) {
                Some(ip)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn is_private(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    match octets {
        [10, _, _, _] => true,
        [172, b, _, _] if (16..=31).contains(&b) => true,
        [192, 168, _, _] => true,
        _ => false,
    }
}
