//! 네트워크 셋업.
//!
//! 현재 M6는 *QEMU user-mode networking* (`-netdev user`)을 사용. guest는
//! QEMU의 내장 SLIRP로 자동으로 10.0.2.x 주소를 받음. DHCP 클라이언트도,
//! 명시적 인터페이스 셋업도 필요 없음. *호스트 측 포트 포워딩만* 설정하면
//! `localhost:<host_port>` → guest의 `0.0.0.0:5550`으로 라우팅.
//!
//! 향후 베어메탈 또는 bridge 네트워크 시점에 실제 DHCP/static IP 셋업 추가.

/// loopback과 eth0 (virtio-net)을 *논리적으로* 올리는 자리. 현 시점에는 no-op.
pub fn bring_up_loopback_and_eth0() -> Result<(), String> {
    println!("[init] network: using QEMU user-mode (auto-configured)");
    Ok(())
}
