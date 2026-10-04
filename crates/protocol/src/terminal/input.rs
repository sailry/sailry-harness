// Adapted from sailry-code 67ae9fa0, terminal_workspace.rs. See third_party_licenses/sailry-code-terminal.md.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InputError {
    #[error("{field} must be non-empty")]
    Empty { field: &'static str },
    #[error("{field} exceeds {max} bytes: {actual}")]
    TooLarge {
        field: &'static str,
        max: usize,
        actual: usize,
    },
    #[error("{field} is invalid")]
    Invalid { field: &'static str },
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Input {
    Text { text: String },
    Focus { focused: bool },
    Key { event: KeyEvent },
    Paste { text: String },
    Mouse { event: MouseEvent },
}

impl Input {
    pub fn validate(&self) -> Result<(), InputError> {
        match self {
            Self::Focus { .. } => Ok(()),
            Self::Text { text } => {
                validate_text("terminal input text", text, MAX_INPUT_BYTES)?;
                if text.chars().any(char::is_control) {
                    return Err(InputError::Invalid {
                        field: "terminal input text",
                    });
                }
                Ok(())
            }
            Self::Key { event } => event.validate(),
            Self::Paste { text } => validate_text("terminal paste", text, MAX_INPUT_BYTES),
            Self::Mouse { event } => event.validate(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MouseEvent {
    pub action: MouseAction,
    pub button: Option<MouseButton>,
    pub modifiers: Modifiers,
    /// Zero-based cell position, independent of fractional font metrics.
    pub column: u16,
    pub row: u16,
    /// Position relative to the grid, in the viewport's pixel coordinate space.
    pub x: u32,
    pub y: u32,
}

impl MouseEvent {
    fn validate(&self) -> Result<(), InputError> {
        if self.column >= MAX_COLUMNS
            || self.row >= MAX_ROWS
            || (self.action != MouseAction::Motion && self.button.is_none())
        {
            return Err(InputError::Invalid {
                field: "terminal mouse event",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEvent {
    pub key: Key,
    pub action: Action,
    pub modifiers: Modifiers,
    pub utf8: Option<String>,
    pub unshifted_codepoint: Option<u32>,
}

impl KeyEvent {
    pub fn unshifted_character(&self) -> Option<char> {
        self.unshifted_codepoint.and_then(key_character)
    }

    pub fn validate(&self) -> Result<(), InputError> {
        self.key.validate()?;
        if let Some(utf8) = self.utf8.as_deref() {
            validate_text("terminal key UTF-8", utf8, MAX_KEY_UTF8_BYTES)?;
            if utf8.chars().any(|character| {
                character.is_control() || matches!(character as u32, 0xF700..=0xF8FF)
            }) {
                return Err(InputError::Invalid {
                    field: "terminal key UTF-8",
                });
            }
        }
        if self
            .unshifted_codepoint
            .is_some_and(|_| self.unshifted_character().is_none())
        {
            return Err(InputError::Invalid {
                field: "terminal key unshifted codepoint",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Key {
    Character { codepoint: u32 },
    Named { key: NamedKey },
    Escape,
    Enter,
    Tab,
    Backspace,
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Function { number: u8 },
}

impl Key {
    pub fn character(self) -> Option<char> {
        match self {
            Self::Character { codepoint } => key_character(codepoint),
            _ => None,
        }
    }

    pub fn function_index(self) -> Option<usize> {
        match self {
            Self::Function { number } => usize::from(number)
                .checked_sub(1)
                .filter(|index| *index < FUNCTION_KEY_COUNT),
            _ => None,
        }
    }

    fn validate(self) -> Result<(), InputError> {
        match self {
            Self::Character { .. } if self.character().is_none() => Err(InputError::Invalid {
                field: "terminal key character",
            }),
            Self::Function { .. } if self.function_index().is_none() => Err(InputError::Invalid {
                field: "terminal function key",
            }),
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Press,
    Repeat,
    Release,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

fn validate_text(field: &'static str, value: &str, max: usize) -> Result<(), InputError> {
    if value.is_empty() {
        return Err(InputError::Empty { field });
    }
    if value.len() > max {
        return Err(InputError::TooLarge {
            field,
            max,
            actual: value.len(),
        });
    }
    Ok(())
}

fn key_character(codepoint: u32) -> Option<char> {
    char::from_u32(codepoint).filter(|character| {
        !character.is_control() && !matches!(*character as u32, 0xF700..=0xF8FF)
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NamedKey {
    AltLeft,
    AltRight,
    ControlLeft,
    ControlRight,
    MetaLeft,
    MetaRight,
    ShiftLeft,
    ShiftRight,
    CapsLock,
    NumLock,
    Fn,
    PrintScreen,
    ScrollLock,
    Pause,
    ContextMenu,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadEnter,
    NumpadEqual,
    NumpadDecimal,
    NumpadComma,
    NumpadClear,
}
