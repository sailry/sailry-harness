use super::*;
use libghostty_vt::terminal::{ClipboardLocation, ClipboardWriteError};
use sailry_protocol::terminal::{Clipboard, Features};

#[derive(Default)]
pub(super) struct Effects {
    bell: u64,
    clipboard: Option<Clipboard>,
}

pub(super) fn register(
    terminal: &mut Ghostty<'static, 'static>,
) -> Result<Rc<RefCell<Effects>>, Fault> {
    let effects = Rc::new(RefCell::new(Effects::default()));
    let bell = effects.clone();
    check(
        terminal.on_bell(move |_| {
            let mut effects = bell.borrow_mut();
            effects.bell = effects.bell.wrapping_add(1);
        }),
        "configure Ghostty bell callback",
    )?;
    let clipboard = effects.clone();
    check(
        terminal.on_clipboard_write(move |_, write| {
            if write.location() != ClipboardLocation::Standard {
                return Err(ClipboardWriteError::Unsupported);
            }
            let text = write
                .contents()
                .find(|content| content.mime.split(';').next() == Some("text/plain"))
                .ok_or(ClipboardWriteError::Unsupported)?
                .data;
            if text.len() > sailry_protocol::terminal::MAX_INPUT_BYTES {
                return Err(ClipboardWriteError::InvalidData);
            }
            let mut effects = clipboard.borrow_mut();
            let sequence = effects
                .clipboard
                .as_ref()
                .map_or(1, |old| old.sequence.wrapping_add(1));
            effects.clipboard = Some(Clipboard {
                sequence,
                text: text.to_owned(),
            });
            Ok(())
        }),
        "configure Ghostty clipboard callback",
    )?;
    Ok(effects)
}

impl Vt {
    pub(super) fn features(&self) -> Result<Features, Fault> {
        let effects = self.effects.borrow();
        Ok(Features {
            focus_reporting: check(
                self.terminal.mode(Mode::FOCUS_EVENT),
                "read Ghostty focus mode",
            )?,
            alternate_scroll: check(
                self.terminal.mode(Mode::ALT_SCROLL),
                "read Ghostty alternate scroll mode",
            )?,
            title: check(self.terminal.title(), "read Ghostty title")?
                .chars()
                .filter(|c| !c.is_control())
                .take(512)
                .collect(),
            directory: check(self.terminal.pwd(), "read Ghostty working directory")?
                .chars()
                .take(8192)
                .collect(),
            bell: effects.bell,
            clipboard: effects.clipboard.clone(),
        })
    }
}
