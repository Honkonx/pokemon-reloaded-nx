use std::{io, net};

/// The original asks the OS for the machine's LAN address with a UDP probe. There is no network
/// stack in this build, so GML sees the loopback address (multiplayer is not supported).
pub fn get_local_ip() -> io::Result<net::IpAddr> {
    Ok(net::IpAddr::V4(net::Ipv4Addr::LOCALHOST))
}
