//! 네트워크 셋업 — loopback과 eth0을 *실제로* 올린다.
//!
//! Linux는 `lo`/`eth0` 둘 다 기본 DOWN 상태로 부팅한다. 평소 systemd/init
//! 스크립트가 처리하지만 우리는 PID 1을 직접 점유하므로 우리가 직접 올려야 한다.
//!
//! - `lo` UP — 같은 VM 안 echo-app → server-host(`127.0.0.1:5550`) 도달용
//! - `eth0` UP + 10.0.2.15/24 — QEMU user-mode 기본 게스트 IP. 호스트 → 포워딩된
//!   포트로 들어온 트래픽이 server-host의 `0.0.0.0:5550`까지 도달하려면 필요.
//!
//! 향후 베어메탈 또는 bridge 네트워크 시점에 DHCP 클라이언트 또는 rtnetlink 기반
//! 풀 구현으로 교체.

use std::io;
use std::mem;
use std::net::Ipv4Addr;
use std::os::fd::AsRawFd;

use nix::libc::{self, c_short, ifreq, sockaddr, sockaddr_in};
use nix::sys::socket::{socket, AddressFamily, SockFlag, SockType};

fn name_to_ifr(name: &str) -> Result<ifreq, String> {
    let bytes = name.as_bytes();
    if bytes.len() >= libc::IFNAMSIZ {
        return Err(format!("interface name too long: {}", name));
    }
    let mut ifr: ifreq = unsafe { mem::zeroed() };
    for (i, b) in bytes.iter().enumerate() {
        ifr.ifr_name[i] = *b as libc::c_char;
    }
    Ok(ifr)
}

fn open_inet_sock() -> Result<std::os::fd::OwnedFd, String> {
    socket(AddressFamily::Inet, SockType::Datagram, SockFlag::empty(), None)
        .map_err(|e| format!("socket(AF_INET): {}", e))
}

fn set_iface_up(name: &str) -> Result<(), String> {
    let sock = open_inet_sock()?;
    let mut ifr = name_to_ifr(name)?;

    // 현재 플래그 조회. SIOC* 상수는 libc 정의가 target에 따라 u64/u32 변동 →
    // 명시 캐스트로 glibc/musl 양쪽 portable.
    let r = unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCGIFFLAGS as libc::Ioctl, &mut ifr) };
    if r < 0 {
        return Err(format!("SIOCGIFFLAGS {}: {}", name, io::Error::last_os_error()));
    }

    // IFF_UP | IFF_RUNNING 비트 추가
    unsafe {
        ifr.ifr_ifru.ifru_flags |= (libc::IFF_UP | libc::IFF_RUNNING) as c_short;
    }

    let r = unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFFLAGS as libc::Ioctl, &ifr) };
    if r < 0 {
        return Err(format!("SIOCSIFFLAGS {}: {}", name, io::Error::last_os_error()));
    }
    Ok(())
}

fn make_sockaddr_in(addr: Ipv4Addr) -> sockaddr_in {
    sockaddr_in {
        sin_family: libc::AF_INET as u16,
        sin_port: 0,
        sin_addr: libc::in_addr { s_addr: u32::from_ne_bytes(addr.octets()) },
        sin_zero: [0; 8],
    }
}

/// `ifru_addr`/`ifru_netmask` (sockaddr) 슬롯에 sockaddr_in을 덮어쓴다.
/// 두 구조체 모두 16바이트이므로 안전.
unsafe fn write_addr_into_union(ifr: &mut ifreq, sa: sockaddr_in, is_netmask: bool) {
    let slot: *mut sockaddr = if is_netmask {
        &mut ifr.ifr_ifru.ifru_netmask as *mut sockaddr
    } else {
        &mut ifr.ifr_ifru.ifru_addr as *mut sockaddr
    };
    std::ptr::write(slot as *mut sockaddr_in, sa);
}

fn set_iface_ipv4(name: &str, addr: Ipv4Addr, netmask: Ipv4Addr) -> Result<(), String> {
    let sock = open_inet_sock()?;

    // address
    {
        let mut ifr = name_to_ifr(name)?;
        unsafe { write_addr_into_union(&mut ifr, make_sockaddr_in(addr), false) };
        let r = unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFADDR as libc::Ioctl, &ifr) };
        if r < 0 {
            return Err(format!("SIOCSIFADDR {} {}: {}", name, addr, io::Error::last_os_error()));
        }
    }

    // netmask
    {
        let mut ifr = name_to_ifr(name)?;
        unsafe { write_addr_into_union(&mut ifr, make_sockaddr_in(netmask), true) };
        let r = unsafe { libc::ioctl(sock.as_raw_fd(), libc::SIOCSIFNETMASK as libc::Ioctl, &ifr) };
        if r < 0 {
            return Err(format!(
                "SIOCSIFNETMASK {} {}: {}",
                name,
                netmask,
                io::Error::last_os_error()
            ));
        }
    }

    Ok(())
}

/// 진단용: 해당 sysfs 경로의 디렉터리 이름 목록 (없으면 빈 벡터).
fn list_sysfs(path: &str) -> Vec<String> {
    std::fs::read_dir(path)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn read_sysfs(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| "(unreadable)".into()).trim().to_string()
}

/// `/sys/class/net`을 폴링해서 loopback 외 첫 번째 인터페이스를 반환.
/// 일부 커널은 PCI/virtio 디바이스 enum이 init 진입 직후엔 미완료 — 짧은 retry 윈도우.
/// 끝까지 못 찾으면 PCI/virtio 버스 상태를 함께 인쇄해 진단을 돕는다.
fn find_primary_iface() -> Option<String> {
    for attempt in 0..10 {
        let names = list_sysfs("/sys/class/net");
        let mut sorted = names.clone();
        sorted.sort();
        if attempt == 0 || attempt == 9 {
            println!("[init] interfaces seen (attempt {}): {:?}", attempt, sorted);
        }
        if let Some(found) = sorted.into_iter().find(|n| n != "lo") {
            return Some(found);
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    // 끝까지 못 찾았으면 PCI/virtio 진단 인쇄
    eprintln!("[init] diagnostic — /sys/bus/pci/devices:");
    for dev in list_sysfs("/sys/bus/pci/devices") {
        let vendor = read_sysfs(&format!("/sys/bus/pci/devices/{}/vendor", dev));
        let devid = read_sysfs(&format!("/sys/bus/pci/devices/{}/device", dev));
        let class = read_sysfs(&format!("/sys/bus/pci/devices/{}/class", dev));
        eprintln!("    {}  vendor={}  device={}  class={}", dev, vendor, devid, class);
    }
    eprintln!("[init] diagnostic — /sys/bus/virtio/devices:");
    for dev in list_sysfs("/sys/bus/virtio/devices") {
        let modalias = read_sysfs(&format!("/sys/bus/virtio/devices/{}/modalias", dev));
        eprintln!("    {}  modalias={}", dev, modalias);
    }
    eprintln!("[init] diagnostic — /sys/bus/virtio/drivers:");
    for drv in list_sysfs("/sys/bus/virtio/drivers") {
        eprintln!("    {}", drv);
    }
    None
}

/// `lo`와 가용한 외부 인터페이스를 UP + IP 셋업.
///
/// `lo`는 즉시 차단 요인 — echo-app이 `127.0.0.1:5550`으로 server-host에 못 붙으면
/// 마운트 트리가 비어버린다. 외부 인터페이스(virtio-net)는 외부 ai-bridge 접속용으로
/// 인터페이스 이름은 커널/버전마다 다를 수 있으므로 `/sys/class/net`에서 발견.
/// 외부 인터페이스 셋업 실패는 *치명적이지 않음* — VM 내부 동작은 계속 가능.
pub fn bring_up_loopback_and_eth0() -> Result<(), String> {
    set_iface_up("lo").map_err(|e| format!("lo UP failed: {}", e))?;
    println!("[init] lo UP");

    let primary = match find_primary_iface() {
        Some(n) => n,
        None => {
            eprintln!("[init] no non-loopback interface found — external connection unavailable");
            return Ok(());
        }
    };

    // QEMU user-mode 기본: guest 10.0.2.15/24, gateway 10.0.2.2. DHCP 없이 static.
    match set_iface_ipv4(&primary, Ipv4Addr::new(10, 0, 2, 15), Ipv4Addr::new(255, 255, 255, 0)) {
        Ok(()) => match set_iface_up(&primary) {
            Ok(()) => println!("[init] {} UP (10.0.2.15/24)", primary),
            Err(e) => eprintln!(
                "[init] {} UP failed: {} — external ai-bridge connection unavailable",
                primary, e
            ),
        },
        Err(e) => eprintln!(
            "[init] {} IP setup failed: {} — external ai-bridge connection unavailable",
            primary, e
        ),
    }
    Ok(())
}
