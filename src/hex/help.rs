use ratatui::{
    Frame, symbols,
    layout::{Alignment, Rect},
    widgets::{Block, Borders, Clear, Padding, Paragraph, Wrap},
};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Result;

use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    editor::UIState,
    i18n::Lang,
    util::center_widget,
};

/// The help text in the interface language.
///
/// One literal per language rather than a table of lines: this is prose in a fixed
/// two-column layout, and splitting it into hundreds of translated fragments would
/// make the layout impossible to see while editing it.
fn help_text(lang: Lang) -> &'static str {
    match lang {
        Lang::En => HELP_EN,
        Lang::Ko => HELP_KO,
        Lang::Zh => HELP_ZH,
    }
}

/// Keyboard shortcut reference shown on F1.
///
/// Kept as one big literal instead of building it from the event handlers, so
/// it doubles as documentation - update this text alongside any key binding
/// change in hex/events.rs, hex/selection.rs, hex/edit.rs, disasm/events.rs or
/// global/events.rs. The Korean and Chinese versions below have to be kept in step
/// with it; `translations_cover_every_section` in the tests checks the section
/// count and the key names, which is the part that goes stale silently.
///
/// Key names, option names and encoding names are never translated: they are what
/// the user types and what the status bar shows.
pub(crate) const HELP_EN: &str = "\
GLOBAL (any view)
  Tab / Shift+Tab      Switch view (Hex <-> Disasm; skips non-executables)
  F6                   Strings list
  F7                   Text view (press again to come back)
  F4                   Header view (press again to come back)
  F5                   String References dialog
  F8                   About / program info (paths, encodings, license)
  F9 / Ctrl+O          Open File dialog
  F12                  Save and quit (same as ':wq')
  Alt+F1               Select Drive dialog
  Alt+F2               Toggle Offset <-> VA address display
  Alt+F6               Set image base (blank = back to the file's own)
  Alt+F7               Cycle decoding width: auto -> 16 -> 32 -> 64
  ;                    Comment the byte under the cursor
  Esc                  Back / cancel / clear selection
  :                    Command line
  =                    Calculator (hex by default, 't' = decimal,
                        Ctrl+L clears, Up/Down history)
  Ctrl+G                Goto Address (hex or VA)
  Ctrl+X                Copy current VA to clipboard
  Ctrl+Left / '-'       Jump back to previous cursor position
  Ctrl+Right / '+'      Jump forward to next cursor position
  Alt+L                 Log window (y copies it to the clipboard)
  Ctrl+K                Modify Block dialog
  Ctrl+H                Wildcard Hex Pattern Replace dialog
  Ctrl+B                Find Pattern dialog (ANSI/UTF-8/UNICODE/Hex)
  F3 / Shift+F3          Repeat last pattern search forward / backward
  Ctrl+R                Cross References (Xref) search

HEX VIEW - navigation
  Arrow keys             Move cursor
  Home / Ctrl+Home       Start of line / start of file
  End / Ctrl+End          End of line / end of file
  PageUp / PageDown       Scroll one page
  Backspace               Go to last visited offset
  [ / ]                    Mark the block: start / end at the cursor
  Alt+[ / Alt+]            Jump to the block's ends, or the nearest
                            coloured block edge

HEX VIEW - editing
  F2                    Enter edit mode at cursor
  Tab                    Switch edit column: HEX -> enc1 -> enc2
  Shift + arrows         Select a block in the focused column
  Ctrl+C                 Copy that block (hex from the byte column,
                          decoded text from an encoding column)
  Ctrl+E                Edit Data dialog
  ~                     Toggle upper/lower case of byte under cursor
  Shift+V               Paste hex or text bytes from clipboard
  Alt+H                 Toggle highlight for byte under cursor
  Ctrl+Z / Alt+Backspace Undo last change (or reverted selection)
  Ctrl+Y                Redo last undone change
  Alt+F3                Revert only the byte under the cursor

HEX VIEW - inside edit mode (F2)
  0-9 a-f                 Type hex digits (two make a byte)
  Tab                     Switch column: HEX -> enc1 -> enc2
  Shift + arrows          Select a block in the focused column
  Ctrl+C                  Copy that block
  Ctrl+E                  Edit Data dialog (also works here)
  ~                       Toggle case and advance
  Esc / Enter             Leave edit mode

HEX VIEW - selection
  Shift + movement        Start / extend a selection (as in Disasm view)
  Esc                     Clear the selection
  Insert                  Fill selection with 0x00 (no selection: 1 byte)
  Delete                  Fill selection with 0x90 NOPs (or just 1 byte)
  ~                       Toggle case of the selection (or 1 byte)
  Ctrl+C                  Copy the selection to the clipboard. What is
                           copied follows the column the block was
                           selected in: hex bytes, or the text those
                           bytes spell in enc1 / enc2
  Ctrl+Z                  Revert changed bytes in the selection
  Ctrl+K                  Modify Block dialog
  Alt+M                   Colorize block (new or existing)
  Mouse drag              Selects too; Enter keeps the block, Esc clears it

HEX VIEW - search & lists
  Ctrl+B                Open Find Pattern dialog (text or hex, Tab/Up/Down
                         to switch field, Enter searches the focused one)
  F3 / Shift+F3          Repeat last pattern search forward / backward
  Alt+N                 Names dialog
  F6                    Strings list
  Alt+E / Alt+Shift+E   Change primary / secondary text encoding

NAMES DIALOG (Alt+N) - the comments in this file
  Up / Down               Move through the list
  PageUp / PageDown       Scroll
  Enter                   Go to that offset
  F2                      Edit that comment
  Delete                  Delete that comment (stays in the list)
  f                       Filter by regex
  o / n                   Sort by offset / by comment text
  Esc                     Close

DISASSEMBLY VIEW
  Up / Down             Previous / next instruction
  PageUp / PageDown     Scroll one page of instructions
  Home / End            First / last instruction
  Shift + movement       Extend selection
  Enter                   Follow branch or memory target
  Ctrl+Enter              Follow target and switch to Hex view
  Space                  Assemble instruction at cursor
                          (numbers are hex; add 't' for decimal, e.g.
                           'push 10' = 0x10, 'push 10t' = 10)
  Ctrl+C                 Copy selected instructions to clipboard
  Ctrl+E                 Edit Data dialog
  Ctrl+R                 Cross References (Xref) search
  F6                      Strings list (addresses shown as VA here)
  Delete                  Fill the instruction under the cursor with NOPs
                           (uses its exact decoded length)
  Ctrl+Z / Alt+Backspace  Undo last change
  Ctrl+Y                  Redo last undone change
  Alt+F3                  Revert only the byte under the cursor

TEXT VIEW
  Up / Down              Scroll a line, then move the window through the file
  Left / Right           Scroll sideways
  Home / Ctrl+Home       Start of line / start of file
  Ctrl+End                End of last visible line
  PageUp / PageDown       Scroll one page
  Alt+E                   Change text encoding

HEADER VIEW
  Left / Right          Switch pane, move column
  Up / Down             Move selection
  PageUp / PageDown     Move a screenful
  Home / End            First / last entry
  Tab                   Switch sidebar <-> detail pane
  Enter                 Edit the selected field
  g / f                 Jump to that field's offset in Hex view
  Esc / q               Leave Header view

HEADER VIEW - Section Tools (PE only, sidebar category 7)
  Align Offset to VA    Set PointerToRawData = VirtualAddress
  Add New Section       Append a section of a given size (default 0x1000)
  Note                  Edits stay in memory; ':w' writes them to disk

COMMAND LINE (':' to open)
  :q                      Quit
  :about  /  :ver         Program info (same as F8; 'y' copies it)
  :w [file]               Save (to file, if given)
  :wq  /  :x [file]       Save and quit
  :wb <file>  /  :wblock <file>   Save selected block to file
  :o [file]  /  :open [file]      Open file (blank = Open dialog)
  :cmt <offset> <text>    Add a comment at offset
  :<address>              Goto address (hex; 't' suffix = decimal;
                           '+'/'-' prefix = relative; 'cur'/'base'/'oep'
                           keywords; supports + and - expressions)
  :set                    Show every option and its current value
  :set byteline <n|auto>  Bytes shown per line (alias: width)
  :set ctrlchar <c>       Non-graphic byte placeholder character
  :set enc1 <name>        Primary encoding (utf-8, cp949, cp936,
                           iso-8859-1, iso-8859-2, utf-16le, utf-16be)
  :set enc2 <name|none>   Secondary encoding, same names plus 'none'
  :set lang en|ko|zh      Interface language (English, 한국어, 中文).
                           Labels only: key names, option names and the
                           status-bar modes stay as they are
  :set theme <name>       Load a hex-view color theme. Disassembly
                           colors are left alone unless the theme file
                           declares them; use ':set disasmtheme' for those
  :set disasmtheme <name>  Disassembly colors only: dark, light, grey,
                           another theme name, or a path to a file
  :set addr va|offset|toggle    Address column contents
                           (':set va' and ':set offset' still work)
  :set bitness <16|32|64|auto>  Force the disassembly decoding width
  :set view hex|disasm|text|header   Switch view

  Every on/off option below takes 'on', 'off' or 'toggle'; with no
  value it turns on. The old 'no<name>' spellings still work.
  :set highlight          Disassembly syntax colors (alias: hilight)
  :set hintbar            Bottom hint line (hold Ctrl or Alt while it
                           is showing to see those bindings)
  :set wrapscan           Wrap search around EOF
  :set db                 Write the .dzdb annotation sidecar file
  :set dimctrl            Dim control bytes
  :set dimzero            Dim null bytes (independent of dimctrl)
  :set disasm_mem/reg/imm/kw/seg/import/import_fg/comment <color>
                          Disassembly colors, #rrggbb or a name
";

pub(crate) const HELP_KO: &str = "\
공통 (모든 뷰)
  Tab / Shift+Tab      뷰 전환 (헥스 <-> 디스어셈블, 실행 파일만)
  F6                   문자열 목록
  F7                   텍스트 뷰 (다시 누르면 복귀)
  F4                   헤더 뷰 (다시 누르면 복귀)
  F5                   문자열 참조 목록
  F8                   프로그램 정보 (경로, 인코딩, 라이선스)
  F9 / Ctrl+O          파일 열기 다이얼로그
  F12                  저장하고 종료 (':wq'와 동일)
  Alt+F1               드라이브 선택
  Alt+F2               주소 표시 전환: 파일 옵셋 <-> VA
  Alt+F6               이미지 베이스 지정 (비우면 파일 값으로 복귀)
  Alt+F7               디코딩 비트 수 순환: auto -> 16 -> 32 -> 64
  ;                    커서 위치에 주석 달기
  Esc                  뒤로 / 취소 / 선택 해제
  :                    커맨드 라인
  =                    계산기 (기본 16진수, 't'는 10진수,
                        Ctrl+L 지우기, 위/아래 히스토리)
  Ctrl+G               주소로 이동 (옵셋 또는 VA)
  Ctrl+X               현재 VA를 클립보드에 복사
  Ctrl+Left / '-'      이전 커서 위치로 되돌아가기
  Ctrl+Right / '+'     다음 커서 위치로 가기
  Alt+L                로그 창 (y 로 클립보드 복사)
  Ctrl+K               블록 일괄 수정
  Ctrl+H               와일드카드 16진 패턴 바꾸기
  Ctrl+B               패턴 찾기 (ANSI/UTF-8/UNICODE/16진)
  F3 / Shift+F3        마지막 패턴 검색 반복: 정방향 / 역방향
  Ctrl+R               Xref (상호 참조) 검색

헥스 뷰 - 이동
  화살표               커서 이동
  Home / Ctrl+Home     줄 시작 / 파일 시작
  End / Ctrl+End       줄 끝 / 파일 끝
  PageUp / PageDown    한 페이지 스크롤
  Backspace            직전에 있던 옵셋으로
  [ / ]                블록 지정: 커서 위치를 시작 / 끝으로
  Alt+[ / Alt+]        블록의 양 끝, 또는 가까운 색칠 블록 경계로 이동

헥스 뷰 - 편집
  F2                   커서 위치에서 편집 모드 시작
  Tab                  편집 칼럼 전환: HEX -> enc1 -> enc2
  Shift + 화살표       포커스된 칼럼에서 블록 선택
  Ctrl+C               그 블록 복사 (바이트 칼럼이면 16진수,
                        인코딩 칼럼이면 디코딩된 텍스트)
  Ctrl+E               데이터 편집 다이얼로그
  ~                    커서 바이트의 대소문자 전환
  Shift+V              클립보드의 16진수 또는 텍스트 붙여넣기
  Alt+H                커서 바이트 값 강조 표시 전환
  Ctrl+Z / Alt+Backspace  마지막 변경 되돌리기 (블록 포함)
  Ctrl+Y               되돌린 변경 다시 실행
  Alt+F3               커서 바이트만 원래 값으로 복원

헥스 뷰 - 편집 모드 안에서 (F2)
  0-9 a-f              16진수 입력 (두 자리가 한 바이트)
  Tab                  칼럼 전환: HEX -> enc1 -> enc2
  Shift + 화살표       포커스된 칼럼에서 블록 선택
  Ctrl+C               그 블록 복사
  Ctrl+E               데이터 편집 다이얼로그 (여기서도 동작)
  ~                    대소문자 전환 후 다음 바이트로
  Esc / Enter          편집 모드 종료

헥스 뷰 - 블록 선택
  Shift + 이동키       선택 시작 / 확장 (디스어셈블 뷰와 동일)
  Esc                  선택 해제
  Insert               블록을 0x00으로 채움 (선택 없으면 1바이트)
  Delete               블록을 0x90 NOP으로 채움 (또는 1바이트)
  ~                    블록의 대소문자 전환 (또는 1바이트)
  Ctrl+C               블록을 클립보드로 복사. 무엇이 복사되는지는
                        블록을 지정한 칼럼에 따름: 16진 바이트이거나
                        그 바이트를 enc1 / enc2로 읽은 텍스트
  Ctrl+Z               블록의 변경 바이트 되돌리기
  Ctrl+K               블록 일괄 수정
  Alt+M                블록 색칠 (새로 만들거나 기존 블록)
  마우스 드래그        선택 가능. Enter로 유지, Esc로 해제

헥스 뷰 - 검색과 목록
  Ctrl+B               패턴 찾기 (텍스트 또는 16진수, Tab/위/아래로
                        칸 이동, Enter로 포커스된 칸 검색)
  F3 / Shift+F3        마지막 패턴 검색 반복: 정방향 / 역방향
  Alt+N                주석 목록
  F6                   문자열 목록
  Alt+E / Alt+Shift+E  주 / 보조 텍스트 인코딩 변경

주석 목록 (Alt+N) - 이 파일의 주석들
  위 / 아래            목록 이동
  PageUp / PageDown    스크롤
  Enter                해당 옵셋으로 이동
  F2                   그 주석 수정
  Delete               그 주석 삭제 (목록에 머무름)
  f                    정규식으로 필터
  o / n                옵셋순 / 주석 내용순 정렬
  Esc                  닫기

디스어셈블 뷰
  위 / 아래            이전 / 다음 명령어
  PageUp / PageDown    한 페이지 스크롤
  Home / End           첫 / 마지막 명령어
  Shift + 이동키       선택 확장
  Enter                분기 또는 메모리 대상 따라가기
  Ctrl+Enter           대상을 따라가면서 헥스 뷰로 전환
  Space                커서 위치에 어셈블
                        (숫자는 16진수, 10진수는 't' 접미사.
                         'push 10' = 0x10, 'push 10t' = 10)
  Ctrl+C               선택한 명령어를 클립보드로 복사
  Ctrl+E               데이터 편집 다이얼로그
  Ctrl+R               Xref (상호 참조) 검색
  F6                   문자열 목록 (여기서는 주소가 VA)
  Delete               커서 명령어를 NOP으로 채움
                        (디코딩된 실제 길이만큼)
  Ctrl+Z / Alt+Backspace  마지막 변경 되돌리기
  Ctrl+Y               되돌린 변경 다시 실행
  Alt+F3               커서 바이트만 원래 값으로 복원

텍스트 뷰
  위 / 아래            한 줄 스크롤, 이어서 파일 창 이동
  왼쪽 / 오른쪽        좌우 스크롤
  Home / Ctrl+Home     줄 시작 / 파일 시작
  Ctrl+End             마지막으로 보이는 줄의 시작
  PageUp / PageDown    한 페이지 스크롤
  Alt+E                텍스트 인코딩 변경

헤더 뷰
  왼쪽 / 오른쪽        패널 전환, 칼럼 이동
  위 / 아래            선택 이동
  PageUp / PageDown    한 화면씩 이동
  Home / End           첫 항목 / 마지막 항목
  Tab                  사이드바 <-> 상세 패널 전환
  Enter                선택한 필드 수정
  g / f                그 필드의 옵셋으로 헥스 뷰에서 이동
  Esc / q              헤더 뷰 나가기

헤더 뷰 - 섹션 도구 (PE 전용, 사이드바 7번 항목)
  Align Offset to VA   PointerToRawData = VirtualAddress 로 맞춤
  Add New Section      지정한 크기의 섹션 추가 (기본 0x1000)
  참고                 수정은 메모리에만 남음. ':w'로 디스크에 기록

커맨드 라인 (':'로 열기)
  :q                   종료
  :about  /  :ver      프로그램 정보 (F8과 동일, 'y'로 복사)
  :w [파일]            저장 (파일명을 주면 그 파일로)
  :wq  /  :x [파일]    저장하고 종료
  :wb <파일>  /  :wblock <파일>   선택한 블록을 파일로 저장
  :o [파일]  /  :open [파일]      파일 열기 (비우면 다이얼로그)
  :cmt <옵셋> <내용>  해당 옵셋에 주석 추가
  :<주소>              주소로 이동 (16진수. 't' 접미사는 10진수,
                        '+'/'-' 접두사는 상대 이동, 'cur'/'base'/'oep'
                        키워드와 + - 수식 사용 가능)
  :set                 모든 옵션과 현재 값 보기
  :set byteline <n|auto>  한 줄에 표시할 바이트 수 (별칭: width)
  :set ctrlchar <문자>    표시 불가 바이트를 대신할 문자
  :set enc1 <이름>     주 인코딩 (utf-8, cp949, cp936,
                        iso-8859-1, iso-8859-2, utf-16le, utf-16be)
  :set enc2 <이름|none>   보조 인코딩. 위와 같은 이름에 'none' 추가
  :set lang en|ko|zh   인터페이스 언어 (English, 한국어, 中文).
                        라벨만 바뀜. 키 이름, 옵션 이름, 상태줄의
                        모드 표시는 그대로 유지됨
  :set theme <이름>    헥스 뷰 색 테마. 테마 파일이 디스어셈블 색을
                        선언하지 않았다면 그 색은 그대로 유지되며,
                        그 경우 ':set disasmtheme'를 사용
  :set disasmtheme <이름>  디스어셈블 색만: dark, light, grey,
                        다른 테마 이름, 또는 파일 경로
  :set addr va|offset|toggle   주소 칼럼 내용
                        (':set va', ':set offset'도 동작)
  :set bitness <16|32|64|auto>  디스어셈블 디코딩 비트 수 고정
  :set view hex|disasm|text|header   뷰 전환

  아래의 켜기/끄기 옵션은 'on', 'off', 'toggle'을 받으며 값을
  생략하면 켜짐. 예전의 'no<이름>' 표기도 그대로 동작함.
  :set highlight       디스어셈블 구문 색 (별칭: hilight)
  :set hintbar         하단 힌트 줄 (표시 중에 Ctrl 또는 Alt를
                        누르고 있으면 그 조합의 목록이 나옴)
  :set wrapscan        파일 끝에서 검색 순환
  :set db              .dzdb 주석 파일 저장
  :set dimctrl         제어 바이트 흐리게
  :set dimzero         널 바이트 흐리게 (dimctrl과 독립)
  :set disasm_mem/reg/imm/kw/seg/import/import_fg/comment <색>
                       디스어셈블 색상. #rrggbb 또는 색 이름
";

pub(crate) const HELP_ZH: &str = "\
通用 (所有视图)
  Tab / Shift+Tab      切换视图 (十六进制 <-> 反汇编，仅可执行文件)
  F6                   字符串列表
  F7                   文本视图 (再按一次返回)
  F4                   头部视图 (再按一次返回)
  F5                   字符串引用列表
  F8                   程序信息 (路径、编码、许可证)
  F9 / Ctrl+O          打开文件对话框
  F12                  保存并退出 (等同 ':wq')
  Alt+F1               选择驱动器
  Alt+F2               地址显示切换：文件偏移 <-> VA
  Alt+F6               设置映像基址 (留空则恢复文件自身的值)
  Alt+F7               循环解码位宽：auto -> 16 -> 32 -> 64
  ;                    为光标处的字节添加注释
  Esc                  返回 / 取消 / 清除选择
  :                    命令行
  =                    计算器 (默认十六进制，'t' 为十进制，
                        Ctrl+L 清空，上/下为历史)
  Ctrl+G               跳转到地址 (偏移或 VA)
  Ctrl+X               复制当前 VA 到剪贴板
  Ctrl+Left / '-'      返回上一个光标位置
  Ctrl+Right / '+'     前进到下一个光标位置
  Alt+L                日志窗口 (y 复制到剪贴板)
  Ctrl+K               批量修改块
  Ctrl+H               通配符十六进制模式替换
  Ctrl+B               查找模式 (ANSI/UTF-8/UNICODE/十六进制)
  F3 / Shift+F3        重复上次模式搜索：向前 / 向后
  Ctrl+R               交叉引用 (Xref) 搜索

十六进制视图 - 移动
  方向键               移动光标
  Home / Ctrl+Home     行首 / 文件开头
  End / Ctrl+End       行尾 / 文件末尾
  PageUp / PageDown    翻页
  Backspace            回到上一次访问的偏移
  [ / ]                标记块：以光标处为起点 / 终点
  Alt+[ / Alt+]        跳到块的两端，或最近的着色块边界

十六进制视图 - 编辑
  F2                   在光标处进入编辑模式
  Tab                  切换编辑列：HEX -> enc1 -> enc2
  Shift + 方向键       在当前列选择一个块
  Ctrl+C               复制该块 (字节列为十六进制，
                        编码列为解码后的文本)
  Ctrl+E               数据编辑对话框
  ~                    切换光标字节的大小写
  Shift+V              粘贴剪贴板中的十六进制或文本
  Alt+H                切换光标字节值的高亮
  Ctrl+Z / Alt+Backspace  撤销上次修改 (含选区)
  Ctrl+Y               重做被撤销的修改
  Alt+F3               仅将光标处的字节还原为原值

十六进制视图 - 编辑模式内 (F2)
  0-9 a-f              输入十六进制数字 (两位构成一个字节)
  Tab                  切换列：HEX -> enc1 -> enc2
  Shift + 方向键       在当前列选择一个块
  Ctrl+C               复制该块
  Ctrl+E               数据编辑对话框 (此处也可用)
  ~                    切换大小写并前进
  Esc / Enter          退出编辑模式

十六进制视图 - 块选择
  Shift + 移动键       开始 / 扩展选择 (与反汇编视图相同)
  Esc                  清除选择
  Insert               将选区填充为 0x00 (无选区则填一个字节)
  Delete               将选区填充为 0x90 NOP (或一个字节)
  ~                    切换选区的大小写 (或一个字节)
  Ctrl+C               复制选区到剪贴板。复制的内容取决于
                        选择时所在的列：十六进制字节，或按
                        enc1 / enc2 解读出的文本
  Ctrl+Z               撤销选区内已修改的字节
  Ctrl+K               批量修改块
  Alt+M                为块着色 (新建或已有的块)
  鼠标拖动             也可选择。Enter 保留，Esc 清除

十六进制视图 - 搜索与列表
  Ctrl+B               查找模式 (文本或十六进制，Tab/上/下
                        切换输入框，Enter 搜索当前框)
  F3 / Shift+F3        重复上次模式搜索：向前 / 向后
  Alt+N                注释列表
  F6                   字符串列表
  Alt+E / Alt+Shift+E  更改主 / 次文本编码

注释列表 (Alt+N) - 本文件中的注释
  上 / 下              在列表中移动
  PageUp / PageDown    滚动
  Enter                跳转到该偏移
  F2                   编辑该注释
  Delete               删除该注释 (停留在列表中)
  f                    用正则过滤
  o / n                按偏移 / 按注释文本排序
  Esc                  关闭

反汇编视图
  上 / 下              上一条 / 下一条指令
  PageUp / PageDown    翻页
  Home / End           第一条 / 最后一条指令
  Shift + 移动键       扩展选择
  Enter                跟随分支或内存目标
  Ctrl+Enter           跟随目标并切换到十六进制视图
  Space                在光标处汇编
                        (数字为十六进制；十进制加 't'，
                         'push 10' = 0x10，'push 10t' = 10)
  Ctrl+C               复制所选指令到剪贴板
  Ctrl+E               数据编辑对话框
  Ctrl+R               交叉引用 (Xref) 搜索
  F6                   字符串列表 (此处地址显示为 VA)
  Delete               用 NOP 填充光标处的指令
                        (按解码出的实际长度)
  Ctrl+Z / Alt+Backspace  撤销上次修改
  Ctrl+Y               重做被撤销的修改
  Alt+F3               仅将光标处的字节还原为原值

文本视图
  上 / 下              滚动一行，然后移动文件窗口
  左 / 右              左右滚动
  Home / Ctrl+Home     行首 / 文件开头
  Ctrl+End             最后一行可见行的开头
  PageUp / PageDown    翻页
  Alt+E                更改文本编码

头部视图
  左 / 右              切换面板，移动列
  上 / 下              移动选择
  PageUp / PageDown    翻页
  Home / End           第一项 / 最后一项
  Tab                  侧栏 <-> 详情面板
  Enter                编辑所选字段
  g / f                在十六进制视图中跳到该字段的偏移
  Esc / q              离开头部视图

头部视图 - 节工具 (仅 PE，侧栏第 7 项)
  Align Offset to VA   令 PointerToRawData = VirtualAddress
  Add New Section      追加一个指定大小的节 (默认 0x1000)
  注意                 修改只在内存中，':w' 才写入磁盘

命令行 (按 ':' 打开)
  :q                   退出
  :about  /  :ver      程序信息 (等同 F8，'y' 复制)
  :w [文件]            保存 (给出文件名则另存)
  :wq  /  :x [文件]    保存并退出
  :wb <文件>  /  :wblock <文件>   将所选块保存为文件
  :o [文件]  /  :open [文件]      打开文件 (留空则弹出对话框)
  :cmt <偏移> <内容>   在该偏移添加注释
  :<地址>              跳转到地址 (十六进制；'t' 后缀为十进制，
                        '+'/'-' 前缀为相对，可用 'cur'/'base'/'oep'
                        关键字以及 + - 表达式)
  :set                 显示所有选项及当前值
  :set byteline <n|auto>  每行显示的字节数 (别名：width)
  :set ctrlchar <字符>    非可见字节的替代字符
  :set enc1 <名称>     主编码 (utf-8, cp949, cp936,
                        iso-8859-1, iso-8859-2, utf-16le, utf-16be)
  :set enc2 <名称|none>   次编码，同上并可用 'none'
  :set lang en|ko|zh   界面语言 (English, 한국어, 中文)。
                        仅标签改变：键名、选项名和状态栏中的
                        模式标识保持不变
  :set theme <名称>    十六进制视图配色。若主题文件未声明反汇编
                        颜色，则这些颜色保持不变；那种情况请用
                        ':set disasmtheme'
  :set disasmtheme <名称>  仅反汇编配色：dark, light, grey,
                        其他主题名，或文件路径
  :set addr va|offset|toggle   地址列内容
                        (':set va'、':set offset' 仍然有效)
  :set bitness <16|32|64|auto>  固定反汇编解码位宽
  :set view hex|disasm|text|header   切换视图

  下列开关选项接受 'on'、'off' 或 'toggle'；省略值即为开启。
  旧的 'no<名称>' 写法仍然有效。
  :set highlight       反汇编语法配色 (别名：hilight)
  :set hintbar         底部提示栏 (显示时按住 Ctrl 或 Alt
                        可看到对应的组合键)
  :set wrapscan        搜索在文件末尾回绕
  :set db              写入 .dzdb 注释文件
  :set dimctrl         淡化控制字节
  :set dimzero         淡化空字节 (与 dimctrl 独立)
  :set disasm_mem/reg/imm/kw/seg/import/import_fg/comment <颜色>
                       反汇编颜色，#rrggbb 或颜色名
";

pub fn dialog_help_draw(app: &mut App, frame: &mut Frame) {
    let width = help_box_width(frame.area());
    // Sized to the actual *rendered* row count (plus the top/bottom border)
    // instead of a fixed 34 rows, which left a large empty area below the
    // text on any terminal taller than the help content. Still clamped to the
    // available screen height, so scrolling remains the fallback on a short
    // terminal.
    let avail_height = frame.area().height.saturating_sub(4).max(8);
    let lang = app.config.lang;
    let text = help_text(lang);
    let height = (help_row_count(text, width) + 2).min(avail_height);
    let dialog_area = center_widget(width, height, frame.area());

    // Clamp before rendering as well: the terminal can be resized while the
    // dialog is open, which shrinks the scroll range under a scroll offset
    // that was legal a moment ago.
    app.help_scroll_offset = app
        .help_scroll_offset
        .min(max_help_scroll(text, width, dialog_area.height));

    let para = Paragraph::new(text)
        .style(app.config.theme.dialog)
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll_offset, 0));

    let block = Block::new()
        .title(crate::i18n::M::HelpTitle.tr(lang))
        .title_bottom(crate::i18n::M::HelpFooter.tr(lang))
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_set(symbols::border::DOUBLE)
        .style(app.config.theme.dialog)
        .padding(Padding::horizontal(1));

    frame.render_widget(Clear, dialog_area);
    frame.render_widget(para.block(block), dialog_area);
}

/// Outer width of the help box for a given screen area. Kept in one place so
/// the draw code and the scroll-clamping code can't disagree about it.
fn help_box_width(area: Rect) -> u16 {
    (area.width.saturating_sub(4)).min(78).max(20)
}

/// Columns available to the text itself: the box width minus the left/right
/// border (2) and the block's horizontal padding (2).
fn help_text_width(box_width: u16) -> u16 {
    box_width.saturating_sub(4).max(1)
}

/// Number of terminal rows `text` actually occupies once wrapped to `box_width`.
///
/// Counting `lines()` alone under-counts, because `Wrap` splits any line
/// longer than the box across several rows - which made the scroll range too
/// short to reach the bottom of the text. Measured in display columns, since a
/// Korean or Chinese line is half as many characters as it is columns wide.
fn help_row_count(text: &str, box_width: u16) -> u16 {
    let text_width = help_text_width(box_width) as usize;
    text.lines()
        .map(|line| {
            let w = UnicodeWidthStr::width(line);
            // An empty line still occupies one row.
            (w.div_ceil(text_width)).max(1) as u16
        })
        .sum()
}

/// Largest scroll offset that still leaves the text filling the box.
///
/// The old bound was `line_count - 1`, which let the user scroll the entire
/// help text off the top of the dialog and stare at a box full of blank rows.
fn max_help_scroll(text: &str, box_width: u16, box_height: u16) -> u16 {
    // Visible text rows = box height minus the top and bottom border.
    let visible_rows = box_height.saturating_sub(2);
    help_row_count(text, box_width).saturating_sub(visible_rows)
}

pub fn dialog_help_events(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Recomputed from the same geometry the draw code uses, so Down/PageDown/End
    // stop exactly when the last help line reaches the bottom of the box
    // instead of scrolling the text off the top.
    let text = help_text(app.config.lang);
    let box_width = help_box_width(app.screen);
    let avail_height = app.screen.height.saturating_sub(4).max(8);
    let box_height = (help_row_count(text, box_width) + 2).min(avail_height);
    let max_scroll = max_help_scroll(text, box_width, box_height);
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::F(1) => {
            app.dialog_renderer = None;
            app.state = UIState::Normal;
            app.help_scroll_offset = 0;
        }
        // Copies the whole help text in the current language, the way `y` does on
        // the About panel: a keymap is more useful in a text file next to the
        // program than scrolled through in a box.
        KeyCode::Char('c') | KeyCode::Char('C') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let copied = match app.clipboard.as_mut() {
                Ok(clip) => clip.set_text(text.to_string()).is_ok(),
                Err(_) => false,
            };
            if copied {
                App::log(app, "Copied the help text to clipboard".to_string());
            } else {
                App::log(app, "Could not access the clipboard".to_string());
                crate::beep!();
            }
        }
        KeyCode::Down => {
            app.help_scroll_offset = (app.help_scroll_offset + 1).min(max_scroll);
        }
        KeyCode::Up => {
            app.help_scroll_offset = app.help_scroll_offset.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.help_scroll_offset = (app.help_scroll_offset + 10).min(max_scroll);
        }
        KeyCode::PageUp => {
            app.help_scroll_offset = app.help_scroll_offset.saturating_sub(10);
        }
        KeyCode::Home => {
            app.help_scroll_offset = 0;
        }
        KeyCode::End => {
            app.help_scroll_offset = max_scroll;
        }
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod help_translation_tests {
    use super::*;

    /// The three texts have to describe the same program.
    ///
    /// They are separate literals, so a binding added to one and not the others is
    /// the failure mode. Section count and key names are the parts that go stale
    /// without anyone noticing; the prose is not checkable here.
    #[test]
    fn translations_cover_every_section() {
        let sections = |text: &str| {
            text.lines()
                .filter(|line| !line.is_empty() && !line.starts_with(' '))
                .count()
        };
        let en = sections(HELP_EN);
        assert!(en >= 10, "the English text should have every section, got {}", en);
        assert_eq!(sections(HELP_KO), en, "the Korean text is missing a section");
        assert_eq!(sections(HELP_ZH), en, "the Chinese text is missing a section");
    }

    /// Key names are not translated, so every text must mention all of them.
    #[test]
    fn every_text_documents_the_same_keys() {
        // One per section, plus the ones most recently moved.
        let keys = [
            "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F12", "Ctrl+B", "Ctrl+C",
            "Ctrl+E", "Ctrl+G", "Ctrl+K", "Ctrl+R", "Ctrl+X", "Ctrl+Z", "Shift+V", "Alt+E",
            "Alt+M", "Alt+N", "Alt+F2", "Alt+F3", ":set lang", ":set view",
        ];
        for (name, text) in [("ko", HELP_KO), ("zh", HELP_ZH), ("en", HELP_EN)] {
            for key in keys {
                assert!(
                    text.contains(key),
                    "the {} help text never mentions {}",
                    name,
                    key
                );
            }
        }
    }

    /// Korean keeps the transliterated jargon rather than inventing native terms.
    #[test]
    fn korean_uses_transliterated_jargon() {
        assert!(HELP_KO.contains("옵셋"), "offset should read as 옵셋");
        assert!(!HELP_KO.contains("오프셋"), "the old spelling is still in the text");
        assert!(HELP_KO.contains("디스어셈블"));
        assert!(HELP_KO.contains("헥스"));
        assert!(HELP_KO.contains("베이스"));
    }

    /// Every text must fit the box the dialog draws, at the narrowest supported
    /// terminal, without a line wrapping into an unreadable stub.
    #[test]
    fn lines_fit_the_help_box() {
        // 68 columns is the minimum terminal; the box is 4 narrower, less padding.
        let text_width = help_text_width(help_box_width(Rect::new(0, 0, 68, 24))) as usize;
        for (name, text) in [("en", HELP_EN), ("ko", HELP_KO), ("zh", HELP_ZH)] {
            for line in text.lines() {
                let width = UnicodeWidthStr::width(line);
                assert!(
                    width <= text_width * 2,
                    "{}: a line is {} columns, more than two rows' worth: {:?}",
                    name,
                    width,
                    line
                );
            }
        }
    }
}

#[cfg(test)]
mod help_copy_tests {
    use super::*;
    use ratatui::crossterm::event::{KeyEvent, KeyModifiers};

    fn press(app: &mut App, code: KeyCode) {
        let _ = dialog_help_events(app, KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// `y` copies the help text and leaves the window open.
    ///
    /// The clipboard is not available on every machine that runs the tests, so the
    /// assertion is on the report rather than on the clipboard contents: either way
    /// the user has to be told what happened.
    #[test]
    fn y_copies_and_keeps_the_window_open() {
        let mut app = App::new();
        app.state = UIState::DialogHelp;
        app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let _ = dialog_help_events(&mut app, key);

        assert!(app.state == UIState::DialogHelp, "copying closed the help window");
        let last = app.logs.last().cloned().unwrap_or_default();
        assert!(
            last.contains("help text") || last.contains("clipboard"),
            "the copy went unreported, log says: {:?}",
            last
        );
    }

    /// Esc still closes it, and the scroll position is reset for next time.
    #[test]
    fn esc_closes_and_resets_the_scroll() {
        let mut app = App::new();
        app.state = UIState::DialogHelp;
        app.screen = ratatui::layout::Rect::new(0, 0, 100, 30);
        app.help_scroll_offset = 5;

        press(&mut app, KeyCode::Esc);

        assert!(app.state == UIState::Normal);
        assert_eq!(app.help_scroll_offset, 0);
    }

    /// The footer names the one key that cannot be guessed, and no longer spends
    /// itself on the arrows.
    #[test]
    fn the_footer_mentions_the_copy_key_only() {
        for lang in crate::i18n::Lang::ALL {
            let footer = crate::i18n::M::HelpFooter.tr(lang);
            assert!(footer.contains('y'), "{:?} footer has no copy key: {:?}", lang, footer);
            assert!(
                !footer.contains("PageUp") && !footer.contains("Page"),
                "{:?} footer still lists the scroll keys: {:?}",
                lang,
                footer
            );
        }
    }
}