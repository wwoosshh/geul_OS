#!/usr/bin/env python3
"""boot/initrd/mkinitrd.py — pure Python cpio newc + gzip initramfs 빌더.

GeulOS M6 빌드 보조. Git Bash/MSYS2/WSL cpio 없이도 initramfs 조립 가능.

사용:
    python mkinitrd.py <stage_dir> <output.cpio.gz>

cpio newc 포맷 사양:
- 각 항목 = 110바이트 헤더(ASCII hex) + 이름(NUL 종료, 4바이트 정렬) + 데이터(4바이트 정렬)
- 마지막 항목 = "TRAILER!!!"
- 디렉터리도 항목으로 포함
"""

from __future__ import annotations

import gzip
import os
import stat
import struct
import sys
from pathlib import Path


def cpio_newc_header(
    ino: int,
    mode: int,
    nlink: int,
    filesize: int,
    name: str,
    is_trailer: bool = False,
) -> bytes:
    """cpio newc 헤더 + 이름 + 패딩 생성."""
    name_with_nul = name + "\0"
    namesize = len(name_with_nul)

    # newc 헤더: 6바이트 매직 + 13개의 8바이트 hex 필드
    header = (
        b"070701"  # magic
        + f"{ino:08x}".encode()
        + f"{mode:08x}".encode()
        + b"00000000"  # uid
        + b"00000000"  # gid
        + f"{nlink:08x}".encode()
        + b"00000000"  # mtime
        + f"{filesize:08x}".encode()
        + b"00000000"  # devmajor
        + b"00000000"  # devminor
        + b"00000000"  # rdevmajor
        + b"00000000"  # rdevminor
        + f"{namesize:08x}".encode()
        + b"00000000"  # check
    )

    assert len(header) == 110, f"header is {len(header)} bytes (expected 110)"

    # 이름 + NUL
    chunk = header + name_with_nul.encode()

    # 이름 끝 4바이트 정렬
    pad = (-len(chunk)) % 4
    chunk += b"\0" * pad

    return chunk


def pad_data(data: bytes) -> bytes:
    """파일 데이터 끝 4바이트 정렬."""
    pad = (-len(data)) % 4
    return data + b"\0" * pad


def assemble_cpio(stage_dir: Path) -> bytes:
    """stage_dir의 모든 파일을 cpio newc 바이트로 직렬화."""
    out = bytearray()
    ino_counter = [721]  # 임의 시작점

    def emit(rel_name: str, file_path: Path) -> None:
        st = file_path.lstat()
        ino_counter[0] += 1
        ino = ino_counter[0]

        if stat.S_ISDIR(st.st_mode):
            mode = stat.S_IFDIR | 0o755
            data = b""
        elif stat.S_ISLNK(st.st_mode):
            mode = stat.S_IFLNK | 0o777
            data = os.readlink(file_path).encode()
        else:
            # 실행 파일은 +x 권한 부여
            mode = stat.S_IFREG | (0o755 if os.access(file_path, os.X_OK) else 0o644)
            data = file_path.read_bytes()

        nlink = 1
        out.extend(cpio_newc_header(ino, mode, nlink, len(data), rel_name))
        out.extend(pad_data(data))

    # 디렉터리 재귀 — 부모 디렉터리 먼저 emit
    entries: list[tuple[str, Path]] = []
    for path in sorted(stage_dir.rglob("*")):
        rel = path.relative_to(stage_dir).as_posix()
        entries.append((rel, path))

    # 디렉터리들을 먼저, 그 다음 파일들 (cpio 일반 관례)
    dirs = [(n, p) for n, p in entries if p.is_dir()]
    files = [(n, p) for n, p in entries if not p.is_dir()]
    for name, path in dirs + files:
        emit(name, path)

    # TRAILER
    out.extend(cpio_newc_header(0, 0, 1, 0, "TRAILER!!!", is_trailer=True))

    return bytes(out)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <stage_dir> <output.cpio.gz>", file=sys.stderr)
        return 2

    stage_dir = Path(sys.argv[1]).resolve()
    output_path = Path(sys.argv[2]).resolve()

    if not stage_dir.is_dir():
        print(f"stage dir not found: {stage_dir}", file=sys.stderr)
        return 1

    print(f"[mkinitrd] assembling cpio newc from {stage_dir}")
    cpio_bytes = assemble_cpio(stage_dir)
    print(f"[mkinitrd] cpio size: {len(cpio_bytes)} bytes")

    print(f"[mkinitrd] gzip → {output_path}")
    with gzip.open(output_path, "wb", compresslevel=6) as f:
        f.write(cpio_bytes)

    final_size = output_path.stat().st_size
    print(f"[mkinitrd] done. final size: {final_size} bytes ({final_size // 1024} KB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
