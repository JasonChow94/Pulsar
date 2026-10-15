//! Network adapters that are up, with their addresses and link speed.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use windows::Win32::NetworkManagement::IpHelper::{
    GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_MULTICAST,
    GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows::Win32::Networking::WinSock::{AF_UNSPEC, SOCKET_ADDRESS};

const ETHERNET: u32 = 6;
const WIFI: u32 = 71;
const ERROR_BUFFER_OVERFLOW: u32 = 111;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetInfo {
    pub name: String,
    pub description: String,
    pub wireless: bool,
    pub mac: Option<String>,
    /// Bits per second.
    pub link_bps: Option<u64>,
    pub ipv4: Vec<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Addr>,
    pub gateway: Option<IpAddr>,
}

fn wide(p: windows::core::PWSTR) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { p.to_string() }.unwrap_or_default()
}

/// The address a `SOCKET_ADDRESS` holds: `sockaddr_in` or `sockaddr_in6`.
pub fn address(raw: &[u8]) -> Option<IpAddr> {
    match u16::from_le_bytes(raw.get(..2)?.try_into().ok()?) {
        2 => Some(IpAddr::V4(<[u8; 4]>::try_from(raw.get(4..8)?).ok()?.into())),
        23 => Some(IpAddr::V6(
            <[u8; 16]>::try_from(raw.get(8..24)?).ok()?.into(),
        )),
        _ => None,
    }
}

fn socket(s: &SOCKET_ADDRESS) -> Option<IpAddr> {
    if s.lpSockaddr.is_null() || s.iSockaddrLength <= 0 {
        return None;
    }
    let raw = unsafe {
        std::slice::from_raw_parts(s.lpSockaddr.cast::<u8>(), s.iSockaddrLength as usize)
    };
    address(raw)
}

pub fn mac(bytes: &[u8]) -> Option<String> {
    (!bytes.is_empty() && bytes.iter().any(|&b| b != 0)).then(|| {
        bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join("-")
    })
}

pub fn read() -> Vec<NetInfo> {
    let flags = GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST;
    let mut size = 16 * 1024u32;
    let mut buffer: Vec<u64>;
    loop {
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        let result = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_UNSPEC.0),
                flags,
                None,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            )
        };
        match result {
            0 => break,
            ERROR_BUFFER_OVERFLOW => continue,
            _ => return Vec::new(),
        }
    }
    let mut out = Vec::new();
    let mut next = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !next.is_null() {
        let a = unsafe { &*next };
        next = a.Next;
        if a.OperStatus != IfOperStatusUp || !matches!(a.IfType, ETHERNET | WIFI) {
            continue;
        }
        let mut info = NetInfo {
            name: wide(a.FriendlyName),
            description: wide(a.Description),
            wireless: a.IfType == WIFI,
            mac: mac(&a.PhysicalAddress[..(a.PhysicalAddressLength as usize).min(8)]),
            link_bps: Some(a.ReceiveLinkSpeed.max(a.TransmitLinkSpeed))
                .filter(|&s| s > 0 && s != u64::MAX),
            ..NetInfo::default()
        };
        let mut unicast = a.FirstUnicastAddress;
        while !unicast.is_null() {
            let u = unsafe { &*unicast };
            match socket(&u.Address) {
                Some(IpAddr::V4(v4)) => info.ipv4.push(v4),
                Some(IpAddr::V6(v6)) => info.ipv6.push(v6),
                None => {}
            }
            unicast = u.Next;
        }
        if !a.FirstGatewayAddress.is_null() {
            info.gateway = socket(unsafe { &(*a.FirstGatewayAddress).Address });
        }
        out.push(info);
    }
    // The adapter that reaches the internet first.
    out.sort_by_key(|n| (n.gateway.is_none(), n.ipv4.is_empty()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_addresses_decode() {
        let mut v4 = vec![2, 0, 0, 0, 192, 168, 1, 20];
        v4.extend([0; 8]);
        assert_eq!(
            address(&v4),
            Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)))
        );
        let mut v6 = vec![23, 0, 0, 0, 0, 0, 0, 0];
        v6.extend(Ipv6Addr::LOCALHOST.octets());
        v6.extend([0; 4]);
        assert_eq!(address(&v6), Some(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert_eq!(address(&[9, 0]), None);
    }

    #[test]
    fn mac_addresses_are_dashed_hex() {
        assert_eq!(
            mac(&[0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]).as_deref(),
            Some("00-1A-2B-3C-4D-5E")
        );
        assert_eq!(mac(&[0; 6]), None);
        assert_eq!(mac(&[]), None);
    }
}
