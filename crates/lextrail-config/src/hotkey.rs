//! Hotkey strings: parsing, the canonical form written to the settings file, the text
//! shown in the settings window, recording from `KeyboardEvent.code`, and the rules that
//! block or warn about a combination (`docs/specs/m1-hotkey-settings.md`, sections 2–3).
//!
//! Platform independent: the desktop app turns a [`Hotkey`] into the global-shortcut
//! plugin's accelerator with [`Hotkey::to_global_shortcut`].

use std::fmt;

/// The modifier keys of a combination. Left and right keys are the same (`RegisterHotKey`
/// does not tell them apart).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    };

    const fn of(ctrl: bool, alt: bool, shift: bool) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            win: false,
        }
    }

    pub fn is_empty(self) -> bool {
        self == Self::NONE
    }

    /// Canonical names in canonical order.
    fn names(self) -> impl Iterator<Item = &'static str> {
        [
            (self.ctrl, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.win, "Win"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
    }
}

/// What kind of key the main key is; the warning rules depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Letter,
    Digit,
    Punctuation,
    /// Space, Enter, Tab, Backspace.
    Typing,
    /// Arrows, Home, End, Page Up / Down, Insert, Delete.
    Navigation,
    Numpad,
    Function,
    Escape,
    /// Caps Lock, Num Lock, Scroll Lock.
    Lock,
    PrintScreen,
    Pause,
}

/// One main key: canonical name (settings file), `KeyboardEvent.code`, display text.
#[derive(Debug, PartialEq, Eq)]
struct KeyInfo {
    name: &'static str,
    code: &'static str,
    display: &'static str,
    kind: Kind,
}

const fn key(name: &'static str, code: &'static str, display: &'static str, kind: Kind) -> KeyInfo {
    KeyInfo {
        name,
        code,
        display,
        kind,
    }
}

macro_rules! letters {
    ($($l:literal)*) => { [$(key($l, concat!("Key", $l), $l, Kind::Letter)),*] };
}
macro_rules! digits {
    ($($d:literal)*) => { [$(key($d, concat!("Digit", $d), $d, Kind::Digit)),*] };
}
macro_rules! function_keys {
    ($($n:literal)*) => {
        [$(key(concat!("F", $n), concat!("F", $n), concat!("F", $n), Kind::Function)),*]
    };
}
macro_rules! numpad_digits {
    ($($d:literal)*) => {
        [$(key(concat!("Num", $d), concat!("Numpad", $d), concat!("小键盘 ", $d), Kind::Numpad)),*]
    };
}

const LETTERS: [KeyInfo; 26] = letters!("A" "B" "C" "D" "E" "F" "G" "H" "I" "J" "K" "L" "M" "N" "O" "P" "Q" "R" "S" "T" "U" "V" "W" "X" "Y" "Z");
const DIGITS: [KeyInfo; 10] = digits!("0" "1" "2" "3" "4" "5" "6" "7" "8" "9");
const FUNCTION_KEYS: [KeyInfo; 24] = function_keys!(
    "1" "2" "3" "4" "5" "6" "7" "8" "9" "10" "11" "12"
    "13" "14" "15" "16" "17" "18" "19" "20" "21" "22" "23" "24"
);
const NUMPAD_DIGITS: [KeyInfo; 10] = numpad_digits!("0" "1" "2" "3" "4" "5" "6" "7" "8" "9");
const OTHER_KEYS: [KeyInfo; 32] = [
    key("Backquote", "Backquote", "`", Kind::Punctuation),
    key("Minus", "Minus", "-", Kind::Punctuation),
    key("Equal", "Equal", "=", Kind::Punctuation),
    key("BracketLeft", "BracketLeft", "[", Kind::Punctuation),
    key("BracketRight", "BracketRight", "]", Kind::Punctuation),
    key("Backslash", "Backslash", "\\", Kind::Punctuation),
    key("Semicolon", "Semicolon", ";", Kind::Punctuation),
    key("Quote", "Quote", "'", Kind::Punctuation),
    key("Comma", "Comma", ",", Kind::Punctuation),
    key("Period", "Period", ".", Kind::Punctuation),
    key("Slash", "Slash", "/", Kind::Punctuation),
    key("Space", "Space", "Space", Kind::Typing),
    key("Enter", "Enter", "Enter", Kind::Typing),
    key("Tab", "Tab", "Tab", Kind::Typing),
    key("Backspace", "Backspace", "Backspace", Kind::Typing),
    key("Insert", "Insert", "Insert", Kind::Navigation),
    key("Delete", "Delete", "Delete", Kind::Navigation),
    key("Home", "Home", "Home", Kind::Navigation),
    key("End", "End", "End", Kind::Navigation),
    key("PageUp", "PageUp", "PageUp", Kind::Navigation),
    key("PageDown", "PageDown", "PageDown", Kind::Navigation),
    key("Up", "ArrowUp", "↑", Kind::Navigation),
    key("Down", "ArrowDown", "↓", Kind::Navigation),
    key("Left", "ArrowLeft", "←", Kind::Navigation),
    key("Right", "ArrowRight", "→", Kind::Navigation),
    key("Escape", "Escape", "Esc", Kind::Escape),
    key("CapsLock", "CapsLock", "CapsLock", Kind::Lock),
    key("NumLock", "NumLock", "NumLock", Kind::Lock),
    key("ScrollLock", "ScrollLock", "ScrollLock", Kind::Lock),
    key(
        "PrintScreen",
        "PrintScreen",
        "PrintScreen",
        Kind::PrintScreen,
    ),
    key("Pause", "Pause", "Pause", Kind::Pause),
    key("NumEnter", "NumpadEnter", "小键盘 Enter", Kind::Numpad),
];
const NUMPAD_OPERATORS: [KeyInfo; 5] = [
    key("NumAdd", "NumpadAdd", "小键盘 +", Kind::Numpad),
    key("NumSubtract", "NumpadSubtract", "小键盘 -", Kind::Numpad),
    key("NumMultiply", "NumpadMultiply", "小键盘 *", Kind::Numpad),
    key("NumDivide", "NumpadDivide", "小键盘 /", Kind::Numpad),
    key("NumDecimal", "NumpadDecimal", "小键盘 .", Kind::Numpad),
];

fn all_keys() -> impl Iterator<Item = &'static KeyInfo> {
    LETTERS
        .iter()
        .chain(&DIGITS)
        .chain(&FUNCTION_KEYS)
        .chain(&NUMPAD_DIGITS)
        .chain(&OTHER_KEYS)
        .chain(&NUMPAD_OPERATORS)
}

/// Extra spellings accepted when reading a hand-edited file (the global-shortcut
/// plugin's names). Matching ignores case.
const ALIASES: [(&str, &str); 9] = [
    ("Esc", "Escape"),
    ("Return", "Enter"),
    ("ArrowUp", "Up"),
    ("ArrowDown", "Down"),
    ("ArrowLeft", "Left"),
    ("ArrowRight", "Right"),
    ("NumPlus", "NumAdd"),
    ("NumpadPlus", "NumAdd"),
    ("PauseBreak", "Pause"),
];

fn find_key(token: &str) -> Option<&'static KeyInfo> {
    let token = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(token))
        .map_or(token, |(_, name)| name);
    all_keys().find(|k| k.name.eq_ignore_ascii_case(token) || k.code.eq_ignore_ascii_case(token))
}

/// A combination: modifiers plus exactly one main key.
#[derive(Clone, PartialEq, Eq)]
pub struct Hotkey {
    pub modifiers: Modifiers,
    key: &'static KeyInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("empty hotkey")]
    Empty,
    #[error("unknown key {0:?}")]
    UnknownKey(String),
    #[error("no main key (only modifiers)")]
    NoKey,
    #[error("more than one main key")]
    MultipleKeys,
    #[error("modifier {0:?} given twice")]
    DuplicateModifier(String),
}

impl Hotkey {
    /// Parses a settings-file string such as `"Ctrl+Alt+X"`, `"ctrl+alt+KeyX"` or `"F8"`.
    /// Case and spaces around `+` do not matter; modifiers may come in any order.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        if text.trim().is_empty() {
            return Err(ParseError::Empty);
        }
        let mut modifiers = Modifiers::NONE;
        let mut main = None;
        for raw in text.split('+') {
            let token = raw.trim();
            let flag = match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some(&mut modifiers.ctrl),
                "alt" | "option" => Some(&mut modifiers.alt),
                "shift" => Some(&mut modifiers.shift),
                "win" | "super" | "meta" | "cmd" | "command" => Some(&mut modifiers.win),
                _ => None,
            };
            if let Some(flag) = flag {
                if std::mem::replace(flag, true) {
                    return Err(ParseError::DuplicateModifier(token.to_owned()));
                }
                continue;
            }
            let key = find_key(token).ok_or_else(|| ParseError::UnknownKey(token.to_owned()))?;
            if main.replace(key).is_some() {
                return Err(ParseError::MultipleKeys);
            }
        }
        let key = main.ok_or(ParseError::NoKey)?;
        Ok(Self { modifiers, key })
    }

    /// A key press seen while recording: `KeyboardEvent.code` and the modifier flags.
    /// `None` when `code` is a modifier itself (the user is still pressing modifiers) or a
    /// key we do not support.
    pub fn from_key_event(code: &str, modifiers: Modifiers) -> Option<Self> {
        all_keys()
            .find(|k| k.code == code)
            .map(|key| Self { modifiers, key })
    }

    /// The canonical form written to the settings file, e.g. `"Ctrl+Alt+X"`.
    pub fn canonical(&self) -> String {
        self.join("+", self.key.name)
    }

    /// The text shown in the settings window, e.g. `"Ctrl + Alt + X"`.
    pub fn display(&self) -> String {
        self.join(" + ", self.key.display)
    }

    /// The accelerator for the global-shortcut plugin (`Win` is `Super` there).
    pub fn to_global_shortcut(&self) -> String {
        self.canonical().replace("Win", "Super")
    }

    fn join(&self, separator: &str, key: &'static str) -> String {
        self.modifiers
            .names()
            .chain(std::iter::once(key))
            .collect::<Vec<_>>()
            .join(separator)
    }

    fn is(&self, modifiers: Modifiers, key_name: &str) -> bool {
        self.modifiers == modifiers && self.key.name == key_name
    }
}

impl fmt::Debug for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hotkey({})", self.canonical())
    }
}

impl fmt::Display for Hotkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical())
    }
}

/// Whether a combination can be used, and what the user should be told first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Cannot be used; the reason is shown in the settings window.
    Blocked(String),
    /// Can be used. Non-empty warnings are shown and must be confirmed before saving.
    Allowed(Vec<String>),
}

/// Combinations LexTrail itself depends on (spec 3.1).
const BLOCKED: [(Modifiers, &str, &str); 3] = [
    (
        Modifiers::of(true, false, false),
        "C",
        "剪贴板兜底要模拟 Ctrl+C 复制选中文字，连按两次 Ctrl+C 也会失效",
    ),
    (
        Modifiers::of(true, false, false),
        "Insert",
        "终端里的剪贴板兜底要模拟 Ctrl+Insert 复制选中文字",
    ),
    (Modifiers::NONE, "Escape", "悬浮窗靠 Esc 关闭"),
];

/// Shortcuts used by (nearly) every program or by Windows itself (spec 3.2, first row).
const COMMON: [(Modifiers, &str, &str); 27] = {
    const CTRL: Modifiers = Modifiers::of(true, false, false);
    const ALT: Modifiers = Modifiers::of(false, true, false);
    const SHIFT: Modifiers = Modifiers::of(false, false, true);
    const CTRL_SHIFT: Modifiers = Modifiers::of(true, false, true);
    [
        (CTRL, "A", "全选"),
        (CTRL, "V", "粘贴"),
        (CTRL, "X", "剪切"),
        (CTRL, "Z", "撤销"),
        (CTRL, "Y", "重做"),
        (CTRL, "S", "保存"),
        (CTRL, "F", "查找"),
        (CTRL, "H", "替换"),
        (CTRL, "P", "打印"),
        (CTRL, "N", "新建"),
        (CTRL, "O", "打开"),
        (CTRL, "W", "关闭标签页或窗口"),
        (CTRL, "T", "新建标签页"),
        (CTRL, "R", "刷新"),
        (CTRL, "Tab", "切换标签页"),
        (CTRL, "Space", "中文输入法的中英文切换"),
        (CTRL, "Period", "中文输入法的中英文标点切换"),
        (CTRL, "Escape", "打开开始菜单"),
        (CTRL, "Backspace", "删除前一个词"),
        (CTRL, "Delete", "删除后一个词"),
        (CTRL_SHIFT, "Z", "重做"),
        (CTRL_SHIFT, "Tab", "切换到上一个标签页"),
        (CTRL_SHIFT, "Escape", "打开任务管理器"),
        (ALT, "F4", "关闭窗口"),
        (ALT, "Tab", "切换窗口"),
        (ALT, "Space", "窗口菜单"),
        (SHIFT, "Space", "中文输入法的全角 / 半角切换"),
    ]
};

/// Combinations well-known programs use (spec section 7): `(program, combination,
/// function)`. Only entries checked against the program's own documentation belong here;
/// the sources are listed in the spec.
const CONFLICTS: &[(&str, &str, &str)] = &[
    // Visual Studio (Microsoft Learn, default keyboard shortcuts): nearly every
    // Ctrl+Alt+letter opens a tool window.
    ("Visual Studio", "Ctrl+Alt+A", "命令窗口"),
    ("Visual Studio", "Ctrl+Alt+B", "断点"),
    ("Visual Studio", "Ctrl+Alt+C", "调用堆栈"),
    ("Visual Studio", "Ctrl+Alt+D", "反汇编"),
    ("Visual Studio", "Ctrl+Alt+E", "异常设置"),
    ("Visual Studio", "Ctrl+Alt+F", "F# Interactive"),
    ("Visual Studio", "Ctrl+Alt+G", "寄存器"),
    ("Visual Studio", "Ctrl+Alt+H", "线程"),
    ("Visual Studio", "Ctrl+Alt+I", "即时窗口"),
    ("Visual Studio", "Ctrl+Alt+J", "对象浏览器"),
    ("Visual Studio", "Ctrl+Alt+K", "调用层次结构"),
    ("Visual Studio", "Ctrl+Alt+L", "解决方案资源管理器"),
    ("Visual Studio", "Ctrl+Alt+N", "克隆查询"),
    ("Visual Studio", "Ctrl+Alt+O", "输出"),
    ("Visual Studio", "Ctrl+Alt+P", "附加到进程"),
    ("Visual Studio", "Ctrl+Alt+Q", "快速监视"),
    ("Visual Studio", "Ctrl+Alt+R", "Web 浏览器"),
    ("Visual Studio", "Ctrl+Alt+S", "服务器资源管理器"),
    ("Visual Studio", "Ctrl+Alt+T", "文档大纲"),
    ("Visual Studio", "Ctrl+Alt+U", "模块"),
    ("Visual Studio", "Ctrl+Alt+X", "工具箱"),
    ("Visual Studio", "Ctrl+Alt+Z", "进程"),
    // IntelliJ IDEA (JetBrains, Windows default keymap).
    ("IntelliJ IDEA", "Ctrl+Alt+A", "接受右侧"),
    ("IntelliJ IDEA", "Ctrl+Alt+B", "转到实现"),
    ("IntelliJ IDEA", "Ctrl+Alt+F", "引入字段"),
    ("IntelliJ IDEA", "Ctrl+Alt+I", "自动缩进"),
    ("IntelliJ IDEA", "Ctrl+Alt+L", "格式化代码"),
    ("IntelliJ IDEA", "Ctrl+Alt+M", "提取方法"),
    ("IntelliJ IDEA", "Ctrl+Alt+N", "内联"),
    ("IntelliJ IDEA", "Ctrl+Alt+P", "引入参数"),
    ("IntelliJ IDEA", "Ctrl+Alt+R", "接受左侧"),
    ("IntelliJ IDEA", "Ctrl+Alt+S", "设置"),
    ("IntelliJ IDEA", "Ctrl+Alt+T", "包围"),
    ("IntelliJ IDEA", "Ctrl+Alt+V", "引入变量"),
    ("IntelliJ IDEA", "Ctrl+Alt+Z", "回滚"),
    ("IntelliJ IDEA", "Alt+Shift+C", "最近更改"),
    ("IntelliJ IDEA", "Alt+Shift+G", "在选中的每行添加光标"),
    ("IntelliJ IDEA", "Alt+Shift+J", "取消最后一个选中项"),
    ("IntelliJ IDEA", "Alt+J", "选中下一个相同项"),
    // Word (Microsoft Support, keyboard shortcuts in Word).
    ("Word", "Ctrl+Alt+1", "标题 1"),
    ("Word", "Ctrl+Alt+2", "标题 2"),
    ("Word", "Ctrl+Alt+3", "标题 3"),
    ("Word", "Ctrl+Alt+C", "复制格式"),
    ("Word", "Ctrl+Alt+D", "插入尾注"),
    ("Word", "Ctrl+Alt+F", "插入脚注"),
    ("Word", "Ctrl+Alt+I", "打印预览"),
    ("Word", "Ctrl+Alt+K", "自动套用格式"),
    ("Word", "Ctrl+Alt+L", "插入 LISTNUM 域"),
    ("Word", "Ctrl+Alt+M", "插入批注"),
    ("Word", "Ctrl+Alt+N", "草稿视图"),
    ("Word", "Ctrl+Alt+O", "大纲视图"),
    ("Word", "Ctrl+Alt+P", "页面视图"),
    ("Word", "Ctrl+Alt+R", "插入 ® 符号"),
    ("Word", "Ctrl+Alt+S", "拆分窗口"),
    ("Word", "Ctrl+Alt+T", "插入 ™ 符号"),
    ("Word", "Ctrl+Alt+V", "粘贴格式"),
    ("Word", "Ctrl+Alt+Z", "返回上次修改的位置"),
    // VS Code (keyboard shortcuts for Windows).
    ("VS Code", "Ctrl+Alt+Up", "在上方添加光标"),
    ("VS Code", "Ctrl+Alt+Down", "在下方添加光标"),
    ("VS Code", "Alt+Shift+F", "格式化文档"),
    ("VS Code", "Alt+Shift+A", "切换块注释"),
    ("VS Code", "Alt+Shift+I", "在每行末尾添加光标"),
    ("VS Code", "Alt+Shift+0", "切换编辑器布局"),
    ("VS Code", "Alt+Shift+Up", "向上复制行"),
    ("VS Code", "Alt+Shift+Down", "向下复制行"),
    ("VS Code", "Alt+Shift+Left", "缩小选区"),
    ("VS Code", "Alt+Shift+Right", "扩大选区"),
    ("VS Code", "Alt+Z", "自动换行"),
    ("VS Code", "Ctrl+Shift+P", "命令面板"),
    ("VS Code", "Ctrl+Shift+N", "新窗口"),
    ("VS Code", "Ctrl+Shift+K", "删除行"),
    ("VS Code", "Ctrl+Shift+L", "选中所有相同项"),
    ("VS Code", "Ctrl+Shift+O", "转到符号"),
    ("VS Code", "Ctrl+Shift+M", "问题面板"),
    ("VS Code", "Ctrl+Shift+E", "资源管理器"),
    ("VS Code", "Ctrl+Shift+F", "全局搜索"),
    ("VS Code", "Ctrl+Shift+G", "源代码管理"),
    ("VS Code", "Ctrl+Shift+D", "调试"),
    ("VS Code", "Ctrl+Shift+X", "扩展"),
    ("VS Code", "Ctrl+Shift+H", "全局替换"),
    ("VS Code", "Ctrl+Shift+U", "输出面板"),
    ("VS Code", "Ctrl+Shift+V", "Markdown 预览"),
    ("VS Code", "Ctrl+Shift+T", "重新打开关闭的编辑器"),
    ("VS Code", "F8", "转到下一个问题"),
    // Chrome (Google Chrome Help, Windows); Edge uses the same ones.
    ("浏览器", "Alt+D", "选中地址栏"),
    ("浏览器", "Alt+E", "打开菜单"),
    ("浏览器", "Alt+F", "打开菜单"),
    ("浏览器", "Ctrl+Shift+N", "新建无痕窗口"),
    ("浏览器", "Ctrl+Shift+T", "重新打开关闭的标签页"),
    ("浏览器", "Ctrl+Shift+W", "关闭窗口"),
    ("浏览器", "Ctrl+Shift+O", "书签管理器"),
    ("浏览器", "Ctrl+Shift+B", "显示书签栏"),
    ("浏览器", "Ctrl+Shift+J", "开发者工具"),
    ("浏览器", "Ctrl+Shift+R", "强制刷新"),
    ("浏览器", "Ctrl+Shift+D", "把所有标签页存为书签"),
    // Windows Terminal (Microsoft Learn, actions).
    ("Windows Terminal", "Ctrl+Alt+1", "切换到第 1 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+2", "切换到第 2 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+3", "切换到第 3 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+4", "切换到第 4 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+5", "切换到第 5 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+6", "切换到第 6 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+7", "切换到第 7 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+8", "切换到第 8 个标签页"),
    ("Windows Terminal", "Ctrl+Alt+Left", "焦点移到上一个窗格"),
    ("Windows Terminal", "Alt+Shift+D", "拆分窗格"),
    ("Windows Terminal", "Alt+Shift+Minus", "向下拆分窗格"),
    ("Windows Terminal", "Alt+Shift+Up", "调整窗格大小"),
    ("Windows Terminal", "Alt+Shift+Down", "调整窗格大小"),
    ("Windows Terminal", "Alt+Shift+Left", "调整窗格大小"),
    ("Windows Terminal", "Alt+Shift+Right", "调整窗格大小"),
    ("Windows Terminal", "Alt+Enter", "全屏"),
    // Foxit PDF Editor (Foxit, keyboard shortcuts for Windows).
    ("福昕 PDF 编辑器", "Ctrl+Alt+V", "原位粘贴"),
    ("福昕 PDF 编辑器", "Ctrl+Alt+D", "复制页面"),
    // SumatraPDF (keyboard shortcuts).
    ("SumatraPDF", "Ctrl+Shift+L", "全屏"),
    ("SumatraPDF", "Ctrl+Shift+N", "在新窗口中打开"),
    ("SumatraPDF", "Ctrl+Shift+S", "把批注保存到 PDF"),
];

/// Decides whether `hotkey` may be used (spec section 3).
pub fn check(hotkey: &Hotkey) -> Verdict {
    if hotkey.modifiers.win {
        return Verdict::Blocked("暂不支持带 Win 键的组合".to_owned());
    }
    if let Some((_, _, why)) = BLOCKED.iter().find(|(m, k, _)| hotkey.is(*m, k)) {
        return Verdict::Blocked(format!("不能使用 {}：{why}", hotkey.display()));
    }
    let mut warnings = Vec::new();
    let shown = hotkey.display();
    if let Some((_, _, function)) = COMMON.iter().find(|(m, k, _)| hotkey.is(*m, k)) {
        warnings.push(format!("所有软件里的「{function}」（{shown}）都会失效"));
    }
    let m = hotkey.modifiers;
    let kind = hotkey.key.kind;
    let types_text = matches!(kind, Kind::Letter | Kind::Digit | Kind::Punctuation)
        || (kind == Kind::Numpad && hotkey.key.name != "NumEnter");
    if m.is_empty() {
        match kind {
            _ if types_text => warnings.push(format!("所有软件里都无法再输入「{shown}」")),
            Kind::Typing | Kind::Navigation | Kind::Numpad => {
                warnings.push(format!("所有软件里的 {shown} 键都会失效"));
            }
            Kind::Lock => warnings.push(format!("{shown} 键会失效")),
            Kind::PrintScreen => warnings.push("系统的截图键会失效".to_owned()),
            Kind::Function
            | Kind::Escape
            | Kind::Pause
            | Kind::Letter
            | Kind::Digit
            | Kind::Punctuation => {}
        }
    } else if m == Modifiers::of(false, false, true) {
        if kind == Kind::Letter {
            warnings.push(format!(
                "所有软件里都无法再输入大写的 {}",
                hotkey.key.display
            ));
        } else if types_text {
            warnings.push(format!("所有软件里都无法再用 {shown} 输入符号"));
        } else if matches!(kind, Kind::Typing | Kind::Navigation) && warnings.is_empty() {
            warnings.push(format!(
                "所有软件里的 {shown}（如扩展选区、反向切换）都会失效"
            ));
        }
    }
    if m.alt && !m.ctrl && !m.shift {
        // D014 supplement: only Alt held, so the Alt mask falls back to 0xE8.
        warnings.push("只有 Alt 的组合在 Windows Terminal 中查不到词（按下时选区会被清除）".to_owned());
    }
    for (program, _, function) in CONFLICTS
        .iter()
        .filter(|(_, combination, _)| Hotkey::parse(combination).as_ref() == Ok(hotkey))
    {
        warnings.push(format!("在 {program} 中是「{function}」，在那里会失效"));
    }
    Verdict::Allowed(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap()
    }

    fn warnings(text: &str) -> Vec<String> {
        match check(&hk(text)) {
            Verdict::Allowed(w) => w,
            Verdict::Blocked(why) => panic!("{text} blocked: {why}"),
        }
    }

    fn blocked(text: &str) -> String {
        match check(&hk(text)) {
            Verdict::Blocked(why) => why,
            Verdict::Allowed(w) => panic!("{text} allowed: {w:?}"),
        }
    }

    const CTRL_ALT: Modifiers = Modifiers::of(true, true, false);

    #[test]
    fn canonical_order_and_names() {
        assert_eq!(hk("ctrl+alt+KeyX").canonical(), "Ctrl+Alt+X");
        assert_eq!(hk("Shift + Alt + Ctrl + x").canonical(), "Ctrl+Alt+Shift+X");
        assert_eq!(hk("f8").canonical(), "F8");
        assert_eq!(hk("Ctrl+Digit1").canonical(), "Ctrl+1");
        assert_eq!(hk("Ctrl+ArrowUp").canonical(), "Ctrl+Up");
        assert_eq!(hk("Alt+esc").canonical(), "Alt+Escape");
        assert_eq!(hk("Ctrl+Alt+Numpad5").canonical(), "Ctrl+Alt+Num5");
        assert_eq!(hk("control+option+minus").canonical(), "Ctrl+Alt+Minus");
        assert_eq!(hk("Super+E").canonical(), "Win+E");
    }

    #[test]
    fn canonical_form_parses_back() {
        for key in all_keys() {
            for text in [key.name.to_owned(), format!("Ctrl+Alt+{}", key.name)] {
                let parsed = hk(&text);
                assert_eq!(parsed.canonical(), text);
                assert_eq!(hk(&parsed.canonical()), parsed);
            }
        }
    }

    #[test]
    fn key_names_and_codes_are_unique() {
        let mut names: Vec<_> = all_keys().map(|k| k.name.to_ascii_lowercase()).collect();
        let mut codes: Vec<_> = all_keys().map(|k| k.code).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!((names.len(), codes.len()), (total, total));
    }

    #[test]
    fn parse_errors() {
        assert_eq!(Hotkey::parse(""), Err(ParseError::Empty));
        assert_eq!(Hotkey::parse("  "), Err(ParseError::Empty));
        assert_eq!(Hotkey::parse("Ctrl+Alt"), Err(ParseError::NoKey));
        assert_eq!(Hotkey::parse("Ctrl+X+Y"), Err(ParseError::MultipleKeys));
        assert_eq!(
            Hotkey::parse("Ctrl+Ctrl+X"),
            Err(ParseError::DuplicateModifier("Ctrl".into()))
        );
        assert_eq!(
            Hotkey::parse("Ctrl+Alt+NotAKey"),
            Err(ParseError::UnknownKey("NotAKey".into()))
        );
        assert_eq!(
            Hotkey::parse("Ctrl++X"),
            Err(ParseError::UnknownKey(String::new()))
        );
    }

    #[test]
    fn display_text() {
        assert_eq!(hk("Ctrl+Alt+X").display(), "Ctrl + Alt + X");
        assert_eq!(hk("Shift+Alt+Up").display(), "Alt + Shift + ↑");
        assert_eq!(hk("Ctrl+Minus").display(), "Ctrl + -");
        assert_eq!(hk("Num7").display(), "小键盘 7");
        assert_eq!(hk("Escape").display(), "Esc");
    }

    #[test]
    fn global_shortcut_accelerator() {
        assert_eq!(hk("Ctrl+Alt+X").to_global_shortcut(), "Ctrl+Alt+X");
        assert_eq!(hk("Win+Shift+T").to_global_shortcut(), "Shift+Super+T");
    }

    #[test]
    fn recording_maps_codes() {
        let rec = |code| Hotkey::from_key_event(code, CTRL_ALT).map(|h| h.canonical());
        assert_eq!(rec("KeyX").as_deref(), Some("Ctrl+Alt+X"));
        assert_eq!(rec("Digit0").as_deref(), Some("Ctrl+Alt+0"));
        assert_eq!(rec("F12").as_deref(), Some("Ctrl+Alt+F12"));
        assert_eq!(rec("ArrowLeft").as_deref(), Some("Ctrl+Alt+Left"));
        assert_eq!(rec("NumpadEnter").as_deref(), Some("Ctrl+Alt+NumEnter"));
        assert_eq!(rec("Slash").as_deref(), Some("Ctrl+Alt+Slash"));
        // Modifiers themselves and unsupported keys: still recording.
        for code in [
            "ControlLeft",
            "AltRight",
            "ShiftLeft",
            "MetaLeft",
            "ContextMenu",
            "IntlBackslash",
            "",
        ] {
            assert_eq!(rec(code), None, "{code}");
        }
        // The code is the physical key; it must match exactly.
        assert_eq!(rec("keyx"), None);
    }

    #[test]
    fn blocked_combinations() {
        assert!(blocked("Ctrl+C").contains("Ctrl+C"));
        assert!(blocked("Ctrl+C").contains("连按两次"));
        assert!(blocked("Ctrl+Insert").contains("终端"));
        assert!(blocked("Escape").contains("Esc 关闭"));
        assert!(blocked("Win+E").contains("Win"));
        assert!(blocked("Ctrl+Alt+Win+X").contains("Win"));
        // Only the exact combinations: the simulated keys carry no other modifier.
        assert!(warnings("Ctrl+Shift+C").is_empty());
        assert!(!warnings("Ctrl+Alt+C").iter().any(|w| w.contains("Ctrl+C")));
        assert!(warnings("Shift+Insert").iter().any(|w| w.contains("Shift")));
        assert!(warnings("Shift+Escape").is_empty());
    }

    #[test]
    fn default_and_typical_choices_have_no_warnings() {
        for text in [
            "Ctrl+Shift+F1",
            "F9",
            "Ctrl+Alt+Y",
            "Pause",
            "Ctrl+Alt+Shift+L",
        ] {
            assert_eq!(warnings(text), Vec::<String>::new(), "{text}");
        }
    }

    #[test]
    fn common_shortcuts_warn_with_their_function() {
        let w = warnings("Ctrl+V");
        assert_eq!(
            w,
            vec!["所有软件里的「粘贴」（Ctrl + V）都会失效".to_owned()]
        );
        for (text, function) in [
            ("Ctrl+X", "剪切"),
            ("Ctrl+Z", "撤销"),
            ("Ctrl+A", "全选"),
            ("Ctrl+S", "保存"),
            ("Ctrl+Shift+Z", "重做"),
            ("Alt+F4", "关闭窗口"),
            ("Ctrl+Space", "中英文切换"),
        ] {
            assert!(
                warnings(text).iter().any(|w| w.contains(function)),
                "{text}"
            );
        }
    }

    #[test]
    fn keys_without_modifiers_warn() {
        assert_eq!(
            warnings("X"),
            vec!["所有软件里都无法再输入「X」".to_owned()]
        );
        assert_eq!(
            warnings("7"),
            vec!["所有软件里都无法再输入「7」".to_owned()]
        );
        assert_eq!(
            warnings("Slash"),
            vec!["所有软件里都无法再输入「/」".to_owned()]
        );
        assert_eq!(
            warnings("Num3"),
            vec!["所有软件里都无法再输入「小键盘 3」".to_owned()]
        );
        assert!(warnings("Space")[0].contains("Space 键都会失效"));
        assert!(warnings("Up")[0].contains("↑ 键都会失效"));
        assert!(warnings("CapsLock")[0].contains("CapsLock"));
        assert!(warnings("PrintScreen")[0].contains("截图"));
        assert!(warnings("F1").is_empty());
        assert!(warnings("F24").is_empty());
    }

    #[test]
    fn shift_only_warns() {
        assert_eq!(
            warnings("Shift+X"),
            vec!["所有软件里都无法再输入大写的 X".to_owned()]
        );
        assert!(warnings("Shift+1")[0].contains("输入符号"));
        assert!(warnings("Shift+Up")[0].contains("扩展选区"));
        // Shift+Space has its own entry; not two warnings.
        assert_eq!(warnings("Shift+Space").len(), 1);
        assert!(warnings("Shift+F8").is_empty());
    }

    #[test]
    fn alt_only_warns_about_windows_terminal() {
        for text in ["Alt+X", "Alt+1", "Alt+F8"] {
            assert!(
                warnings(text)
                    .iter()
                    .any(|w| w.contains("Windows Terminal")),
                "{text}"
            );
        }
        for text in ["Ctrl+Alt+Q", "Alt+Shift+X", "Ctrl+Shift+X"] {
            assert!(
                !warnings(text)
                    .iter()
                    .any(|w| w.contains("Windows Terminal")),
                "{text}"
            );
        }
    }

    #[test]
    fn several_warnings_are_all_listed() {
        // Alt+Space: window menu and Alt only.
        assert_eq!(warnings("Alt+Space").len(), 2);
    }

    #[test]
    fn listed_conflicts_name_the_program() {
        assert_eq!(
            warnings("Ctrl+Alt+X"),
            vec!["在 Visual Studio 中是「工具箱」，在那里会失效".to_owned()]
        );
        // Several programs, each listed.
        let w = warnings("Ctrl+Alt+L");
        assert!(w.iter().any(|w| w.contains("IntelliJ IDEA")));
        assert!(w.iter().any(|w| w.contains("Word")));
        assert!(w.iter().any(|w| w.contains("Visual Studio")));
    }

    #[test]
    fn rule_tables_parse() {
        for (m, k, _) in BLOCKED.iter().chain(&COMMON) {
            assert!(find_key(k).is_some(), "{k}");
            assert!(!m.win);
        }
        for (program, combination, _) in CONFLICTS {
            let hotkey = Hotkey::parse(combination).unwrap();
            assert_eq!(
                hotkey.canonical(),
                *combination,
                "{program}: write it canonically"
            );
        }
    }
}
