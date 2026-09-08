use crate::settings::{Action, Editor, Handle};
use anyhow::Result;
use crossterm::event::KeyEvent;
use futures_util::FutureExt;
use ratatui::Frame;

pub(super) struct Panel {
    handle: Handle,
    draft: crate::settings::Draft,
    editor: Editor,
    saving: Option<tokio::task::JoinHandle<Result<()>>>,
}

impl Panel {
    pub fn open(handle: Handle) -> Result<Self> {
        let draft = handle.draft()?;
        let mut editor = Editor::new(draft.config.clone(), false)?;
        editor.notice = "Saved defaults apply to subsequent work; explicit launch/assignment overrides still take precedence. Ctrl+C cancels running work.".into();
        Ok(Self {
            handle,
            draft,
            editor,
            saving: None,
        })
    }

    pub fn poll(&mut self) -> bool {
        self.editor.poll();
        if self.saving.as_ref().is_some_and(|job| job.is_finished()) {
            let result = self.saving.take().expect("save job").now_or_never();
            match result {
                Some(Ok(Ok(()))) => return true,
                Some(Ok(Err(error))) => self.editor.notice = format!("Not applied: {error}"),
                _ => {
                    self.editor.notice =
                        "Save failed; reopen Settings to inspect the saved revision.".into()
                }
            }
        }
        false
    }

    pub fn key(&mut self, key: KeyEvent) -> Result<bool> {
        if self.saving.is_some() {
            return Ok(false);
        }
        match self.editor.key(key)? {
            Action::Stay => {}
            Action::Cancel => return Ok(true),
            Action::Save => {
                self.draft.config = self.editor.config.clone();
                let mut draft = self.draft.clone();
                let handle = self.handle.clone();
                self.editor.notice = "Saving private settings…".into();
                self.saving = Some(tokio::task::spawn_blocking(move || handle.save(&mut draft)));
            }
        }
        Ok(false)
    }

    pub fn draw(&self, frame: &mut Frame) {
        self.editor.draw(frame, frame.area());
    }

    pub fn paste(&mut self, text: &str) {
        if self.saving.is_none() {
            self.editor.paste(text);
        }
    }
}
