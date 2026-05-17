#!/usr/bin/env bash
# boot/qemu/launch.sh — QEMU로 GeulOS VM 부팅 (Linux/Mac host)
#
# 사전:
# - boot/kernel/vmlinuz 준비됨
# - boot/initrd/geulos.cpio.gz 빌드됨
# - qemu-system-x86_64 설치됨

set -euo pipefail

WORKSPACE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
KERNEL="${KERNEL:-${WORKSPACE_ROOT}/boot/kernel/vmlinuz}"
INITRD="${INITRD:-${WORKSPACE_ROOT}/boot/initrd/geulos.cpio.gz}"
FORWARD_PORT="${FORWARD_PORT:-5550}"
MEMORY="${MEMORY:-512}"

# 사전 점검
if [[ ! -f "$KERNEL" ]]; then
    echo "kernel missing: $KERNEL" >&2
    echo "Download from: https://dl-cdn.alpinelinux.org/alpine/v3.21/releases/x86_64/netboot/vmlinuz-lts" >&2
    exit 1
fi
if [[ ! -f "$INITRD" ]]; then
    echo "initrd missing: $INITRD" >&2
    echo "Build with: pwsh boot/build.ps1 -Release   (or equivalent)" >&2
    exit 1
fi

# KVM 사용 가능 여부
ACCEL="kvm"
if [[ ! -e /dev/kvm ]]; then
    ACCEL="tcg"
    echo "[warn] KVM not available, falling back to TCG (slow)" >&2
fi

echo ""
echo "=== Boot GeulOS in QEMU ==="
echo "kernel:    $KERNEL"
echo "initrd:    $INITRD"
echo "memory:    ${MEMORY}M"
echo "accel:     $ACCEL"
echo "forward:   host :$FORWARD_PORT  →  guest :5550"
echo ""
echo "Console below. ai-bridge can connect via 127.0.0.1:$FORWARD_PORT"
echo "Press Ctrl+A then X to quit QEMU."
echo ""

exec qemu-system-x86_64 \
    -kernel "$KERNEL" \
    -initrd "$INITRD" \
    -m "${MEMORY}M" \
    -accel "$ACCEL" \
    -nographic \
    -append "console=ttyS0 quiet" \
    -netdev "user,id=net0,hostfwd=tcp::${FORWARD_PORT}-:5550" \
    -device virtio-net-pci,netdev=net0
