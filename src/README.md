# src/ — 글 OS 소스

페이즈 0 W12에 `kernel.gl` 작성 시작 (빈 무한루프부터).

## 페이즈별 추가 예정

- **페이즈 0**: `kernel.gl` (한 줄 커널)
- **페이즈 1**: `boot.gl`/`boot.asm` (32→64 전환 스텁), `vga.gl`, `serial.gl`, `idt.gl`, `keyboard.gl`, `timer.gl`
- **페이즈 2**: `phys_alloc.gl`, `vm.gl`, `heap.gl`, `sip.gl`, 시연 SIP들
- **페이즈 3**: `scheduler.gl`, `ipc.gl`, `channel.gl`, `capability.gl`
- **페이즈 4**: `disk_driver.gl`, `fs.gl`, `vfs.gl`, `checkpoint.gl`
- **페이즈 5**: `shell.gl`, `editor.gl`

세부 작업 흐름은 `docs/phases/*.md` 참조.
