# tools/ — 빌드·디버그 보조 도구

페이즈 진행하며 추가:

- **페이즈 0**: `bochsrc.txt` (Bochs 설정 템플릿)
- **페이즈 1**: `serial_monitor.ps1` (시리얼 출력 모니터), `gdb_init.gdb` (GDB 자동화 스크립트)
- **페이즈 2+**: ELF 검사 헬퍼, 페이지 테이블 dump 도구 등

`build.ps1`은 루트에 있고, 이 폴더는 그것이 호출하는 보조 도구 모음.
