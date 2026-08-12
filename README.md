# dezes (dz6)

**dezes**는 Rust로 개발된 초고속 터미널 헥사(Hex) 에디터, PE 파일 분석기 및 디스어셈블러/어셈블러입니다.

---

## ✨ 주요 기능 (Key Features)

- ⚡ **초고속 터미널 TUI (Terminal UI)**
  - Rust 언어로 작성되어 대용량 바이너리도 지연 없이 빠르게 탐색 및 편집할 수 있습니다.
- 🔬 **x86 / x64 디스어셈블리 & 어셈블리**
  - 바이트 단위 디스어셈블리 뷰, Xref 참조 확인, 코드 주석 추가 및 어셈블(Assemble) 기능을 제공합니다.
- 🎨 **풍부한 테마 지원 (Rich Themes)**
  - 메인 테마: `dark`, `light`, `gray`, `latte`, `arctic_ice`, `coffee`, `darkone`, `ice`, `matrix`, `mocha`, `paper`, `punk`, `terracotta` 등
  - 디스어셈블리 테마: `disasm`, `dark`, `light`, `grey` 등 지원 (`:set theme <name>`, `:set disasmtheme <name>`)
- 🌐 **다국어 지원 (i18n)**
  - 한국어(Ko), 영어(En), 중국어(Zh) 3개 국어를 완벽히 지원하며, 명령어로 자유롭게 언어를 전환할 수 있습니다 (`:set lang ko`).
- ⚙️ **자동 설정 파일 (`.dzsrc`)**
  - 실행 폴더에 `.dzsrc` 설정 파일이 없는 경우 자동으로 생성하고, 언어, 인코딩, 테마 설정을 보존합니다.

---

## 🛠️ 빌드 및 실행 방법 (Build & Run)

### 필수 요구사항
- [Rust & Cargo](https://www.rust-lang.org/) (최신 stable 버전을 권장합니다)

### 소스코드 빌드
```bash
# 개발 빌드 및 테스트
cargo test

# 릴리스 프로필 최적화 빌드
cargo build --release
```

빌드가 완료되면 `target/release/dezes.exe` (Windows) 바이너리가 생성됩니다.

---

## ⌨️ 기본 명령어 및 단축키 (Commands & Shortcuts)

- `:set lang <ko|en|zh>` : 언어 설정 변경
- `:set theme <name>` : 메인 Hex UI 테마 변경
- `:set disasmtheme <name>` : 디스어셈블리 UI 테마 변경
- `:q` / `:quit` : 종료

---

## 📄 라이선스 (License)

이 프로젝트는 MIT 라이선스를 따릅니다.
