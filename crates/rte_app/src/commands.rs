//! Command registry: every action reachable from the palette or a key.
//! Keys and the palette dispatch through the same [`Action`].

use std::sync::OnceLock;

use rte_platform::Key;

/// Windows virtual-key codes.
pub mod vk {
    pub const VK_BACK: u16 = 0x08;
    pub const VK_TAB: u16 = 0x09;
    pub const VK_RETURN: u16 = 0x0D;
    pub const VK_ESCAPE: u16 = 0x1B;
    pub const VK_SPACE: u16 = 0x20;
    pub const VK_PRIOR: u16 = 0x21;
    pub const VK_NEXT: u16 = 0x22;
    pub const VK_END: u16 = 0x23;
    pub const VK_HOME: u16 = 0x24;
    pub const VK_LEFT: u16 = 0x25;
    pub const VK_UP: u16 = 0x26;
    pub const VK_RIGHT: u16 = 0x27;
    pub const VK_DOWN: u16 = 0x28;
    pub const VK_INSERT: u16 = 0x2D;
    pub const VK_DELETE: u16 = 0x2E;
    pub const VK_0: u16 = 0x30;
    pub const VK_1: u16 = 0x31;
    pub const VK_A: u16 = 0x41;
    pub const VK_C: u16 = 0x43;
    pub const VK_D: u16 = 0x44;
    pub const VK_F: u16 = 0x46;
    pub const VK_G: u16 = 0x47;
    pub const VK_H: u16 = 0x48;
    pub const VK_I: u16 = 0x49;
    pub const VK_K: u16 = 0x4B;
    pub const VK_L: u16 = 0x4C;
    pub const VK_M: u16 = 0x4D;
    pub const VK_N: u16 = 0x4E;
    pub const VK_O: u16 = 0x4F;
    pub const VK_P: u16 = 0x50;
    pub const VK_R: u16 = 0x52;
    pub const VK_S: u16 = 0x53;
    pub const VK_U: u16 = 0x55;
    pub const VK_V: u16 = 0x56;
    pub const VK_W: u16 = 0x57;
    pub const VK_X: u16 = 0x58;
    pub const VK_Y: u16 = 0x59;
    pub const VK_Z: u16 = 0x5A;
    pub const VK_ADD: u16 = 0x6B;
    pub const VK_SUBTRACT: u16 = 0x6D;
    pub const VK_F1: u16 = 0x70;
    pub const VK_F2: u16 = 0x71;
    pub const VK_F3: u16 = 0x72;
    pub const VK_F4: u16 = 0x73;
    pub const VK_OEM_PLUS: u16 = 0xBB;
    pub const VK_OEM_MINUS: u16 = 0xBD;
    /// `/` and `?` on US keyboards.
    pub const VK_OEM_2: u16 = 0xBF;
    /// `\` and `|` on US keyboards.
    pub const VK_OEM_5: u16 = 0xDC;
}

use vk::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Save,
    Close,
    ToggleReadOnly,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    DeleteWordLeft,
    DeleteWordRight,
    Outdent,
    ToggleOvertype,
    SelectAll,
    SelectToTop,
    SelectToBottom,
    GoToTop,
    GoToBottom,
    ScrollLineUp,
    ScrollLineDown,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ShowCommands,
    ShowFavorites,
    ToggleSyntax,
    About,
    KeyboardShortcuts,
    Documentation,
    Quit,
    Find,
    Replace,
    FindNext,
    FindPrevious,
    ReplaceOne,
    ReplaceAll,
    SelectAllMatches,
    ToggleMatchCase,
    ToggleWholeWord,
    ToggleRegex,
    ToggleGlob,
    TogglePreserveCase,
    ToggleFindInSelection,
    AddNextOccurrence,
    MoveToNextOccurrence,
    SelectAllOccurrences,
    ChangeAllOccurrences,
    AddCursorAbove,
    AddCursorBelow,
    CursorsAtLineEnds,
    ExpandLineSelection,
    CursorUndo,
    ColumnSelectUp,
    ColumnSelectDown,
    ColumnSelectLeft,
    ColumnSelectRight,
    ColumnSelectPageUp,
    ColumnSelectPageDown,
    ChangeLanguage,
    ToggleLineComment,
    ToggleBlockComment,
    GoToBracket,
    ExpandSelection,
    ShrinkSelection,
    GoToSymbol,
}

/// A key with exactly these modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub vk: u16,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

const fn plain(vk: u16) -> Binding {
    Binding {
        vk,
        ctrl: false,
        shift: false,
        alt: false,
    }
}

const fn shift(vk: u16) -> Binding {
    Binding {
        shift: true,
        ..plain(vk)
    }
}

const fn ctrl(vk: u16) -> Binding {
    Binding {
        ctrl: true,
        ..plain(vk)
    }
}

const fn ctrl_shift(vk: u16) -> Binding {
    Binding {
        ctrl: true,
        shift: true,
        ..plain(vk)
    }
}

const fn alt(vk: u16) -> Binding {
    Binding {
        alt: true,
        ..plain(vk)
    }
}

const fn ctrl_alt(vk: u16) -> Binding {
    Binding {
        ctrl: true,
        alt: true,
        ..plain(vk)
    }
}

const fn shift_alt(vk: u16) -> Binding {
    Binding {
        shift: true,
        alt: true,
        ..plain(vk)
    }
}

const fn ctrl_shift_alt(vk: u16) -> Binding {
    Binding {
        ctrl: true,
        shift: true,
        alt: true,
        ..plain(vk)
    }
}

pub struct CommandDef {
    /// VS Code command ID where one exists.
    pub id: &'static str,
    /// Palette label: `Family: Title`.
    pub label: &'static str,
    pub keys: &'static [Binding],
    /// Two-key sequences (`Ctrl+K Ctrl+D`).
    pub chords: &'static [(Binding, Binding)],
    /// Refused while the document is locked.
    pub mutates: bool,
    pub action: Action,
}

const fn command(
    id: &'static str,
    label: &'static str,
    keys: &'static [Binding],
    mutates: bool,
    action: Action,
) -> CommandDef {
    CommandDef {
        id,
        label,
        keys,
        chords: &[],
        mutates,
        action,
    }
}

const fn chord(
    id: &'static str,
    label: &'static str,
    chords: &'static [(Binding, Binding)],
    action: Action,
) -> CommandDef {
    CommandDef {
        id,
        label,
        keys: &[],
        chords,
        mutates: false,
        action,
    }
}

/// All commands, sorted by label.
pub static COMMANDS: &[CommandDef] = &[
    command(
        "workbench.action.showAboutDialog",
        "Application: About",
        &[],
        false,
        Action::About,
    ),
    command(
        "workbench.action.openDocumentationUrl",
        "Application: Documentation",
        &[],
        false,
        Action::Documentation,
    ),
    command(
        "workbench.action.keybindingsReference",
        "Application: Keyboard Shortcuts",
        &[],
        false,
        Action::KeyboardShortcuts,
    ),
    command(
        "workbench.action.quit",
        "Application: Quit",
        &[alt(VK_F4)],
        false,
        Action::Quit,
    ),
    command(
        "editor.action.clipboardCopyAction",
        "Edit: Copy",
        &[ctrl(VK_C)],
        false,
        Action::Copy,
    ),
    command(
        "editor.action.clipboardCutAction",
        "Edit: Cut",
        &[ctrl(VK_X)],
        true,
        Action::Cut,
    ),
    command(
        "deleteWordLeft",
        "Edit: Delete Word Left",
        &[ctrl(VK_BACK)],
        true,
        Action::DeleteWordLeft,
    ),
    command(
        "deleteWordRight",
        "Edit: Delete Word Right",
        &[ctrl(VK_DELETE)],
        true,
        Action::DeleteWordRight,
    ),
    command(
        "editor.action.outdentLines",
        "Edit: Outdent Line",
        &[shift(VK_TAB)],
        true,
        Action::Outdent,
    ),
    command(
        "editor.action.clipboardPasteAction",
        "Edit: Paste",
        &[ctrl(VK_V)],
        true,
        Action::Paste,
    ),
    command(
        "redo",
        "Edit: Redo",
        &[ctrl(VK_Y), ctrl_shift(VK_Z)],
        true,
        Action::Redo,
    ),
    command(
        "editor.action.blockComment",
        "Edit: Toggle Block Comment",
        &[shift_alt(VK_A)],
        true,
        Action::ToggleBlockComment,
    ),
    command(
        "editor.action.commentLine",
        "Edit: Toggle Line Comment",
        &[ctrl(VK_OEM_2)],
        true,
        Action::ToggleLineComment,
    ),
    command(
        "editor.action.toggleOvertypeInsertMode",
        "Edit: Toggle Overtype Mode",
        &[plain(VK_INSERT)],
        false,
        Action::ToggleOvertype,
    ),
    command("undo", "Edit: Undo", &[ctrl(VK_Z)], true, Action::Undo),
    command(
        "workbench.action.closeActiveEditor",
        "File: Close",
        &[ctrl(VK_W), ctrl(VK_F4)],
        false,
        Action::Close,
    ),
    command(
        "workbench.action.files.save",
        "File: Save",
        &[ctrl(VK_S)],
        false,
        Action::Save,
    ),
    command(
        "workbench.action.files.toggleActiveEditorReadonlyInSession",
        "File: Toggle Read-Only",
        &[],
        false,
        Action::ToggleReadOnly,
    ),
    command(
        "cursorTop",
        "Go: Beginning of File",
        &[ctrl(VK_HOME)],
        false,
        Action::GoToTop,
    ),
    command(
        "cursorBottom",
        "Go: End of File",
        &[ctrl(VK_END)],
        false,
        Action::GoToBottom,
    ),
    command(
        "editor.action.jumpToBracket",
        "Go: Go to Bracket",
        &[ctrl_shift(VK_OEM_5)],
        false,
        Action::GoToBracket,
    ),
    command(
        "workbench.action.gotoSymbol",
        "Go: Go to Symbol in Editor",
        &[ctrl_shift(VK_O)],
        false,
        Action::GoToSymbol,
    ),
    command(
        "actions.find",
        "Search: Find",
        &[ctrl(VK_F)],
        false,
        Action::Find,
    ),
    command(
        "editor.action.nextMatchFindAction",
        "Search: Find Next",
        &[plain(VK_F3)],
        false,
        Action::FindNext,
    ),
    command(
        "editor.action.previousMatchFindAction",
        "Search: Find Previous",
        &[shift(VK_F3)],
        false,
        Action::FindPrevious,
    ),
    command(
        "editor.action.startFindReplaceAction",
        "Search: Replace",
        &[ctrl(VK_H)],
        false,
        Action::Replace,
    ),
    command(
        "editor.action.replaceAll",
        "Search: Replace All",
        &[ctrl_alt(VK_RETURN)],
        true,
        Action::ReplaceAll,
    ),
    command(
        "editor.action.replaceOne",
        "Search: Replace Next",
        &[ctrl_shift(VK_1)],
        true,
        Action::ReplaceOne,
    ),
    command(
        "editor.action.selectAllMatches",
        "Search: Select All Matches",
        &[alt(VK_RETURN)],
        false,
        Action::SelectAllMatches,
    ),
    command(
        "toggleFindInSelection",
        "Search: Toggle Find in Selection",
        &[alt(VK_L)],
        false,
        Action::ToggleFindInSelection,
    ),
    command(
        "royi.toggleFindGlob",
        "Search: Toggle Glob",
        &[alt(VK_G)],
        false,
        Action::ToggleGlob,
    ),
    command(
        "toggleFindCaseSensitive",
        "Search: Toggle Match Case",
        &[alt(VK_C)],
        false,
        Action::ToggleMatchCase,
    ),
    command(
        "togglePreserveCase",
        "Search: Toggle Preserve Case",
        &[alt(VK_P)],
        false,
        Action::TogglePreserveCase,
    ),
    command(
        "toggleFindRegex",
        "Search: Toggle Regular Expression",
        &[alt(VK_R)],
        false,
        Action::ToggleRegex,
    ),
    command(
        "toggleFindWholeWord",
        "Search: Toggle Whole Word",
        &[alt(VK_W)],
        false,
        Action::ToggleWholeWord,
    ),
    command(
        "editor.action.insertCursorAbove",
        "Selection: Add Cursor Above",
        &[ctrl_alt(VK_UP)],
        false,
        Action::AddCursorAbove,
    ),
    command(
        "editor.action.insertCursorBelow",
        "Selection: Add Cursor Below",
        &[ctrl_alt(VK_DOWN)],
        false,
        Action::AddCursorBelow,
    ),
    command(
        "editor.action.insertCursorAtEndOfEachLineSelected",
        "Selection: Add Cursors to Line Ends",
        &[shift_alt(VK_I)],
        false,
        Action::CursorsAtLineEnds,
    ),
    command(
        "editor.action.addSelectionToNextFindMatch",
        "Selection: Add Next Occurrence",
        &[ctrl(VK_D)],
        false,
        Action::AddNextOccurrence,
    ),
    command(
        "editor.action.changeAll",
        "Selection: Change All Occurrences",
        &[ctrl(VK_F2)],
        false,
        Action::ChangeAllOccurrences,
    ),
    command(
        "cursorColumnSelectDown",
        "Selection: Column Select Down",
        &[ctrl_shift_alt(VK_DOWN)],
        false,
        Action::ColumnSelectDown,
    ),
    command(
        "cursorColumnSelectLeft",
        "Selection: Column Select Left",
        &[ctrl_shift_alt(VK_LEFT)],
        false,
        Action::ColumnSelectLeft,
    ),
    command(
        "cursorColumnSelectPageDown",
        "Selection: Column Select Page Down",
        &[ctrl_shift_alt(VK_NEXT)],
        false,
        Action::ColumnSelectPageDown,
    ),
    command(
        "cursorColumnSelectPageUp",
        "Selection: Column Select Page Up",
        &[ctrl_shift_alt(VK_PRIOR)],
        false,
        Action::ColumnSelectPageUp,
    ),
    command(
        "cursorColumnSelectRight",
        "Selection: Column Select Right",
        &[ctrl_shift_alt(VK_RIGHT)],
        false,
        Action::ColumnSelectRight,
    ),
    command(
        "cursorColumnSelectUp",
        "Selection: Column Select Up",
        &[ctrl_shift_alt(VK_UP)],
        false,
        Action::ColumnSelectUp,
    ),
    command(
        "cursorUndo",
        "Selection: Cursor Undo",
        &[ctrl(VK_U)],
        false,
        Action::CursorUndo,
    ),
    command(
        "expandLineSelection",
        "Selection: Expand Line Selection",
        &[ctrl(VK_L)],
        false,
        Action::ExpandLineSelection,
    ),
    command(
        "editor.action.smartSelect.expand",
        "Selection: Expand Selection",
        &[shift_alt(VK_RIGHT)],
        false,
        Action::ExpandSelection,
    ),
    chord(
        "editor.action.moveSelectionToNextFindMatch",
        "Selection: Move Last Selection to Next Occurrence",
        &[(ctrl(VK_K), ctrl(VK_D))],
        Action::MoveToNextOccurrence,
    ),
    command(
        "editor.action.selectAll",
        "Selection: Select All",
        &[ctrl(VK_A)],
        false,
        Action::SelectAll,
    ),
    command(
        "editor.action.selectHighlights",
        "Selection: Select All Occurrences",
        &[ctrl_shift(VK_L)],
        false,
        Action::SelectAllOccurrences,
    ),
    command(
        "cursorTopSelect",
        "Selection: Select to Beginning of File",
        &[ctrl_shift(VK_HOME)],
        false,
        Action::SelectToTop,
    ),
    command(
        "cursorBottomSelect",
        "Selection: Select to End of File",
        &[ctrl_shift(VK_END)],
        false,
        Action::SelectToBottom,
    ),
    command(
        "editor.action.smartSelect.shrink",
        "Selection: Shrink Selection",
        &[shift_alt(VK_LEFT)],
        false,
        Action::ShrinkSelection,
    ),
    chord(
        "workbench.action.editor.changeLanguageMode",
        "View: Change Language Mode",
        &[(ctrl(VK_K), plain(VK_M))],
        Action::ChangeLanguage,
    ),
    command(
        "editor.action.fontZoomReset",
        "View: Reset Zoom",
        &[ctrl(VK_0)],
        false,
        Action::ZoomReset,
    ),
    command(
        "scrollLineDown",
        "View: Scroll Line Down",
        &[ctrl(VK_DOWN)],
        false,
        Action::ScrollLineDown,
    ),
    command(
        "scrollLineUp",
        "View: Scroll Line Up",
        &[ctrl(VK_UP)],
        false,
        Action::ScrollLineUp,
    ),
    command(
        "workbench.action.showCommands",
        "View: Show All Commands",
        &[ctrl_shift(VK_P), plain(VK_F1)],
        false,
        Action::ShowCommands,
    ),
    command(
        "royi.showFavoriteCommands",
        "View: Show Favorite Commands",
        &[ctrl(VK_P)],
        false,
        Action::ShowFavorites,
    ),
    command(
        "royi.toggleSyntaxHighlighting",
        "View: Toggle Syntax Highlighting",
        &[],
        false,
        Action::ToggleSyntax,
    ),
    command(
        "editor.action.fontZoomIn",
        "View: Zoom In",
        &[ctrl(VK_OEM_PLUS), ctrl_shift(VK_OEM_PLUS), ctrl(VK_ADD)],
        false,
        Action::ZoomIn,
    ),
    command(
        "editor.action.fontZoomOut",
        "View: Zoom Out",
        &[ctrl(VK_OEM_MINUS), ctrl(VK_SUBTRACT)],
        false,
        Action::ZoomOut,
    ),
];

/// Keys handled by the editor itself, listed in the shortcuts reference.
pub static EDITING_KEYS: &[(&str, &str)] = &[
    ("Move the caret", "Arrows, Home, End, PgUp, PgDn"),
    ("Move by word", "Ctrl+Left, Ctrl+Right"),
    ("Extend the selection", "Shift + any move"),
    ("Delete", "Backspace, Delete"),
    ("New line", "Enter"),
    ("Indent", "Tab"),
    ("Collapse selections", "Escape"),
    ("Select word / line", "Double / triple click"),
    ("Add or remove a cursor", "Alt+Click"),
    ("Column selection", "Shift+Alt+Drag, middle-button drag"),
    ("Zoom", "Ctrl+Wheel"),
];

#[cfg(test)]
pub fn by_id(id: &str) -> Option<&'static CommandDef> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// Registry index of the command `id`.
pub fn index_of(id: &str) -> Option<usize> {
    COMMANDS.iter().position(|c| c.id == id)
}

/// Text of the first key of each command (palette hints), built once.
pub fn key_hint(index: usize) -> &'static str {
    static HINTS: OnceLock<Vec<String>> = OnceLock::new();
    let hints = HINTS.get_or_init(|| {
        COMMANDS
            .iter()
            .map(|c| match (c.keys.first(), c.chords.first()) {
                (Some(&b), _) => key_text(b),
                (None, Some(&pair)) => chord_text(pair),
                (None, None) => String::new(),
            })
            .collect()
    });
    hints.get(index).map_or("", String::as_str)
}

fn binding(key: Key) -> Binding {
    Binding {
        vk: key.vk,
        ctrl: key.ctrl,
        shift: key.shift,
        alt: key.alt,
    }
}

/// `Ctrl+K` style text of a pressed key.
pub fn key_name(key: Key) -> String {
    key_text(binding(key))
}

pub fn for_key(key: Key) -> Option<&'static CommandDef> {
    let binding = binding(key);
    COMMANDS.iter().find(|c| c.keys.contains(&binding))
}

/// Whether `key` starts a chord.
pub fn starts_chord(key: Key) -> bool {
    let binding = binding(key);
    COMMANDS
        .iter()
        .any(|c| c.chords.iter().any(|(first, _)| *first == binding))
}

/// The command of the chord `first`, `second`.
pub fn for_chord(first: Key, second: Key) -> Option<&'static CommandDef> {
    let pair = (binding(first), binding(second));
    COMMANDS.iter().find(|c| c.chords.contains(&pair))
}

/// `Ctrl+K Ctrl+D` style text.
pub fn chord_text((first, second): (Binding, Binding)) -> String {
    format!("{} {}", key_text(first), key_text(second))
}

/// `Ctrl+Shift+P` style text.
pub fn key_text(b: Binding) -> String {
    let mut text = String::new();
    if b.ctrl {
        text.push_str("Ctrl+");
    }
    if b.shift {
        text.push_str("Shift+");
    }
    if b.alt {
        text.push_str("Alt+");
    }
    let name = match b.vk {
        VK_BACK => "Backspace",
        VK_TAB => "Tab",
        VK_RETURN => "Enter",
        VK_ESCAPE => "Escape",
        VK_SPACE => "Space",
        VK_PRIOR => "PgUp",
        VK_NEXT => "PgDn",
        VK_END => "End",
        VK_HOME => "Home",
        VK_LEFT => "Left",
        VK_UP => "Up",
        VK_RIGHT => "Right",
        VK_DOWN => "Down",
        VK_INSERT => "Insert",
        VK_DELETE => "Delete",
        VK_ADD => "Numpad+",
        VK_SUBTRACT => "Numpad-",
        VK_OEM_PLUS => "=",
        VK_OEM_MINUS => "-",
        VK_OEM_2 => "/",
        VK_OEM_5 => "\\",
        0x30..=0x39 | 0x41..=0x5A => {
            text.push(char::from(b.vk as u8));
            return text;
        }
        0x70..=0x87 => {
            text.push_str(&format!("F{}", b.vk - 0x6F));
            return text;
        }
        _ => "?",
    };
    text.push_str(name);
    text
}

/// All keys of a command, comma separated.
pub fn keys_text(def: &CommandDef) -> String {
    def.keys
        .iter()
        .map(|&b| key_text(b))
        .chain(def.chords.iter().map(|&pair| chord_text(pair)))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parses `Ctrl+Shift+P` style text, the inverse of [`key_text`].
pub fn parse_key(text: &str) -> Option<Key> {
    let (mut rest, mut ctrl, mut shift, mut alt) = (text.trim(), false, false, false);
    loop {
        if let Some(r) = rest.strip_prefix("Ctrl+") {
            (rest, ctrl) = (r, true);
        } else if let Some(r) = rest.strip_prefix("Shift+") {
            (rest, shift) = (r, true);
        } else if let Some(r) = rest.strip_prefix("Alt+") {
            (rest, alt) = (r, true);
        } else {
            break;
        }
    }
    let vk = (1..=0xFE).find(|&vk| key_text(plain(vk)) == rest)?;
    Some(Key {
        vk,
        ctrl,
        shift,
        alt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_sorted_unique_and_grouped_in_families() {
        for pair in COMMANDS.windows(2) {
            assert!(
                pair[0].label < pair[1].label,
                "{} >= {}",
                pair[0].label,
                pair[1].label
            );
        }
        let families = [
            "Application",
            "Edit",
            "File",
            "Go",
            "Search",
            "Selection",
            "View",
        ];
        for (i, c) in COMMANDS.iter().enumerate() {
            let family = c.label.split_once(": ").map(|(f, _)| f);
            assert!(families.contains(&family.unwrap_or("")), "{}", c.label);
            assert!(
                COMMANDS[i + 1..]
                    .iter()
                    .all(|o| o.id != c.id && o.action != c.action),
                "{} is registered twice",
                c.id
            );
            for b in c.keys {
                let owner = for_key(Key {
                    vk: b.vk,
                    ctrl: b.ctrl,
                    shift: b.shift,
                    alt: b.alt,
                });
                assert_eq!(
                    owner.map(|o| o.id),
                    Some(c.id),
                    "{} is bound twice",
                    key_text(*b)
                );
                assert!(!key_text(*b).contains('?'), "{} has no key name", c.id);
                assert!(!starts_chord(Key {
                    vk: b.vk,
                    ctrl: b.ctrl,
                    shift: b.shift,
                    alt: b.alt
                }));
            }
            for &(first, second) in c.chords {
                let key = |b: Binding| Key {
                    vk: b.vk,
                    ctrl: b.ctrl,
                    shift: b.shift,
                    alt: b.alt,
                };
                assert!(for_key(key(first)).is_none(), "{} prefix is bound", c.id);
                assert_eq!(for_chord(key(first), key(second)).map(|o| o.id), Some(c.id));
            }
        }
    }

    #[test]
    fn keys_match_modifiers_exactly() {
        let key = |vk, ctrl, shift, alt| Key {
            vk,
            ctrl,
            shift,
            alt,
        };
        assert_eq!(
            for_key(key(VK_Z, true, false, false)).map(|c| c.action),
            Some(Action::Undo)
        );
        assert_eq!(
            for_key(key(VK_Z, true, true, false)).map(|c| c.action),
            Some(Action::Redo)
        );
        assert_eq!(
            for_key(key(VK_P, true, true, false)).map(|c| c.action),
            Some(Action::ShowCommands)
        );
        assert_eq!(
            for_key(key(VK_P, true, false, false)).map(|c| c.action),
            Some(Action::ShowFavorites)
        );
        assert!(for_key(key(VK_S, true, false, true)).is_none());
        assert_eq!(
            for_key(key(VK_F4, false, false, true)).map(|c| c.action),
            Some(Action::Quit)
        );
        assert_eq!(
            for_key(key(VK_F4, true, false, false)).map(|c| c.action),
            Some(Action::Close)
        );
        assert!(for_key(key(VK_LEFT, false, false, false)).is_none());
    }

    #[test]
    fn key_names() {
        assert_eq!(
            keys_text(by_id("workbench.action.showCommands").unwrap()),
            "Ctrl+Shift+P, F1"
        );
        assert_eq!(
            keys_text(by_id("editor.action.fontZoomIn").unwrap()),
            "Ctrl+=, Ctrl+Shift+=, Ctrl+Numpad+"
        );
        assert_eq!(key_text(ctrl(VK_BACK)), "Ctrl+Backspace");
        assert_eq!(key_text(ctrl(VK_0)), "Ctrl+0");
        assert_eq!(key_text(alt(VK_F4)), "Alt+F4");
        assert_eq!(
            keys_text(by_id("editor.action.moveSelectionToNextFindMatch").unwrap()),
            "Ctrl+K Ctrl+D"
        );
        assert_eq!(
            key_hint(index_of("editor.action.replaceAll").unwrap()),
            "Ctrl+Alt+Enter"
        );
        assert_eq!(key_text(ctrl_shift(VK_1)), "Ctrl+Shift+1");
        for c in COMMANDS {
            for &b in c.keys {
                let key = parse_key(&key_text(b)).unwrap();
                assert_eq!(
                    (key.vk, key.ctrl, key.shift, key.alt),
                    (b.vk, b.ctrl, b.shift, b.alt)
                );
            }
        }
        assert_eq!(parse_key("Escape").map(|k| k.vk), Some(VK_ESCAPE));
        assert!(parse_key("Ctrl+Nothing").is_none());
    }
}
