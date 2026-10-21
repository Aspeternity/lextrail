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

/// Whether a combination can be used. Like other tools with global hotkeys (D048),
/// LexTrail only refuses combinations that cannot work; what other programs do with a
/// combination is not checked: a combination another program has registered fails to
/// register, and the user is told so then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Cannot be used; the reason is shown in the settings window.
    Blocked(String),
    Allowed,
}

/// Combinations LexTrail itself depends on (spec 3.1).
const OWN: [(Modifiers, &str, &str); 3] = [
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

/// Combinations Windows keeps for itself (spec 3.1): they switch or close windows, open
/// the Start menu or Task Manager, or switch the input method. F12 alone is reserved
/// for debuggers (`RegisterHotKey` documentation).
const SYSTEM: [(Modifiers, &str); 8] = {
    const CTRL: Modifiers = Modifiers::of(true, false, false);
    const ALT: Modifiers = Modifiers::of(false, true, false);
    [
        (ALT, "Tab"),
        (ALT, "F4"),
        (ALT, "Escape"),
        (CTRL, "Escape"),
        (Modifiers::of(true, false, true), "Escape"),
        (Modifiers::of(true, true, false), "Delete"),
        (CTRL, "Space"),
        (Modifiers::NONE, "F12"),
    ]
};

/// Decides whether `hotkey` may be used (spec section 3).
pub fn check(hotkey: &Hotkey) -> Verdict {
    let shown = hotkey.display();
    if hotkey.modifiers.win {
        return Verdict::Blocked("暂不支持带 Win 键的组合".to_owned());
    }
    if let Some((_, _, why)) = OWN.iter().find(|(m, k, _)| hotkey.is(*m, k)) {
        return Verdict::Blocked(format!("不能使用 {shown}：{why}"));
    }
    if SYSTEM.iter().any(|(m, k)| hotkey.is(*m, k)) {
        return Verdict::Blocked(format!("不能使用 {shown}：这是 Windows 保留的组合"));
    }
    // Without Ctrl or Alt only function keys and Pause: anything else is typed, or
    // moves the cursor or the selection, in every program.
    let m = hotkey.modifiers;
    let free_key = matches!(hotkey.key.kind, Kind::Function | Kind::Pause);
    if !m.ctrl && !m.alt && !free_key {
        let why = if m.shift {
            "只按 Shift 的组合会影响输入大写字母、符号或选择文字，请加上 Ctrl 或 Alt"
        } else {
            "单独的按键会影响正常输入，请加上 Ctrl 或 Alt"
        };
        return Verdict::Blocked(format!("不能使用 {shown}：{why}"));
    }
    Verdict::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hk(text: &str) -> Hotkey {
        Hotkey::parse(text).unwrap()
    }

    fn allowed(text: &str) -> bool {
        check(&hk(text)) == Verdict::Allowed
    }

    fn blocked(text: &str) -> String {
        match check(&hk(text)) {
            Verdict::Blocked(why) => why,
            Verdict::Allowed => panic!("{text} allowed"),
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
    fn own_combinations_are_blocked() {
        assert!(blocked("Ctrl+C").contains("Ctrl+C"));
        assert!(blocked("Ctrl+C").contains("连按两次"));
        assert!(blocked("Ctrl+Insert").contains("终端"));
        assert!(blocked("Escape").contains("Esc 关闭"));
        // Only the exact combinations: the simulated keys carry no other modifier.
        assert!(allowed("Ctrl+Shift+C"));
        assert!(allowed("Ctrl+Alt+C"));
        assert!(allowed("Ctrl+Alt+Insert"));
    }

    #[test]
    fn win_is_not_supported() {
        assert!(blocked("Win+E").contains("Win"));
        assert!(blocked("Ctrl+Alt+Win+X").contains("Win"));
    }

    #[test]
    fn system_combinations_are_blocked() {
        for text in [
            "Alt+Tab",
            "Alt+F4",
            "Alt+Escape",
            "Ctrl+Escape",
            "Ctrl+Shift+Escape",
            "Ctrl+Alt+Delete",
            "Ctrl+Space",
            "F12",
        ] {
            assert!(blocked(text).contains("Windows 保留"), "{text}");
        }
        // With other modifiers they are ordinary combinations.
        assert!(allowed("Ctrl+F12"));
        assert!(allowed("Ctrl+Alt+Tab"));
        assert!(allowed("Ctrl+Alt+Space"));
    }

    #[test]
    fn keys_without_ctrl_or_alt_are_blocked() {
        for text in [
            "X",
            "7",
            "Slash",
            "Num3",
            "Space",
            "Enter",
            "Up",
            "CapsLock",
            "PrintScreen",
        ] {
            assert!(blocked(text).contains("单独的按键"), "{text}");
        }
        for text in [
            "Shift+X",
            "Shift+1",
            "Shift+Up",
            "Shift+Space",
            "Shift+Insert",
        ] {
            assert!(blocked(text).contains("只按 Shift"), "{text}");
        }
        for text in ["F1", "F8", "F24", "Pause", "Shift+F8", "Shift+Pause"] {
            assert!(allowed(text), "{text}");
        }
    }

    #[test]
    fn everything_else_is_allowed_without_comment() {
        // Common shortcuts of other programs are the user's choice (D048).
        for text in [
            "Ctrl+Alt+X",
            "Alt+Space",
            "Alt+X",
            "Ctrl+V",
            "Ctrl+Shift+F1",
            "Ctrl+Alt+L",
            "Ctrl+Alt+Shift+L",
            "Alt+Shift+X",
        ] {
            assert!(allowed(text), "{text}");
        }
    }

    #[test]
    fn rule_tables_name_known_keys() {
        for (m, k) in OWN
            .iter()
            .map(|(m, k, _)| (m, k))
            .chain(SYSTEM.iter().map(|(m, k)| (m, k)))
        {
            assert!(find_key(k).is_some(), "{k}");
            assert!(!m.win);
        }
    }
}
