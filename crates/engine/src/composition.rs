//! IME preedit lives outside the document. Only a commit edits the real presentation, once.

use std::sync::Arc;

use deckcraft_model::{ShapeKind, edit as ed};
use serde_json::{Value, json};

use crate::{DocState, Result, Selection, Session, TextSel};

#[derive(Clone, Debug)]
pub struct Composition {
    uid: u64,
    revision: u64,
    selection: Selection,
    /// A disposable document used only for canvas rendering and caret layout.
    pub preview: DocState,
    pub start: ed::Pos,
    pub end: ed::Pos,
}

impl Composition {
    fn matches(&self, st: &DocState) -> bool {
        self.uid == st.uid && self.revision == st.revision && self.selection == st.selection
    }
}

impl Session {
    /// A composition becomes invalid if another command, pointer gesture or document switch
    /// changes its target. Never insert a late commit into a different document or selection.
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref().filter(|c| self.active().is_some_and(|st| c.matches(st)))
    }

    pub(crate) fn preedit_text(&mut self, text: &str) -> Result<Value> {
        if text.is_empty() {
            self.composition = None;
            return Ok(json!({"handled": true}));
        }
        // Input methods only need short strings; reject unbounded control/MCP input.
        if text.len() > 1 << 20 {
            return Err(crate::cmd::bad("text.preedit", "composition exceeds 1 MiB"));
        }
        let st = self.doc()?;
        let mut ts = match st.selection.text.clone() {
            Some(ts) => ts,
            None => {
                let Some(id) = st.selection.shapes.first().copied().filter(|_| st.selection.shapes.len() == 1) else {
                    return Ok(json!({"handled": false}));
                };
                let Some(sh) = st.shape(id).filter(|sh| matches!(sh.kind, ShapeKind::Shape | ShapeKind::Connector { .. })) else {
                    return Ok(json!({"handled": false}));
                };
                // Like type_text: typing on a selected shape replaces its whole text.
                TextSel { shape: id, caret: sh.text.as_ref().map(ed::end).unwrap_or((0, 0)), ..Default::default() }
            }
        };
        let mut preview = st.clone();
        preview.history = Default::default();
        preview.interaction = None;
        let (start, finish) = ts.ordered();
        let body = crate::cmd::text::body_mut(Arc::make_mut(&mut preview.doc), &preview.selection, &ts)
            .ok_or_else(|| crate::cmd::bad("text.preedit", "the text being edited is gone"))?;
        let at = ed::delete(body, start, finish);
        let end = ed::insert(body, at, text, None);
        ts.anchor = end;
        ts.caret = end;
        preview.selection.text = Some(ts);
        self.composition = Some(Composition { uid: st.uid, revision: st.revision, selection: st.selection.clone(), preview, start: at, end });
        Ok(json!({"handled": true}))
    }

    pub(crate) fn commit_text(&mut self, text: &str) -> Result<Value> {
        if text.len() > 1 << 20 {
            return Err(crate::cmd::bad("text.commit", "composition exceeds 1 MiB"));
        }
        let stale = self.composition.is_some() && self.composition().is_none();
        self.composition = None;
        if stale || text.is_empty() || matches!(text, "\n" | "\r") {
            return Ok(json!({"handled": false}));
        }
        self.type_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editing(text: &str) -> Session {
        let mut s = Session::with_new();
        let id = s.doc().unwrap().current_slide().unwrap().shapes[0].id;
        s.execute("text.set", &json!({"id": id, "text": text})).unwrap();
        s.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
        s
    }

    fn text(st: &DocState) -> String {
        crate::cmd::text::body_of(st, st.selection.text.as_ref().unwrap()).unwrap().text()
    }

    #[test]
    fn hangul_preedit_replaces_preview_and_commits_once() {
        let mut s = editing("");
        let doc = s.doc().unwrap().doc.clone();
        let undo = s.doc().unwrap().history.undo.len();
        let journal = s.journal.len();
        s.doc_mut().unwrap().saved_doc = doc.clone();
        for candidate in ["ㅇ", "아", "안", "안ㄴ", "안녀", "안녕", "안녕하세요"] {
            s.execute("text.preedit", &json!({"text": candidate})).unwrap();
            assert_eq!(text(&s.composition().unwrap().preview), candidate);
            assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
            assert!(!s.doc().unwrap().is_dirty());
            assert_eq!(s.doc().unwrap().history.undo.len(), undo);
            assert_eq!(s.journal.len(), journal);
        }
        s.execute("text.commit", &json!({"text": "안녕하세요"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "안녕하세요");
        assert!(s.composition().is_none());
        assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "");
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "안녕하세요");
    }

    #[test]
    fn cancellation_preserves_selected_text_and_formatting() {
        let mut s = editing("기존 제목");
        s.execute("text.selectAll", &json!({})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        for cancel in ["text.cancelComposition", "text.preedit", "text.commit"] {
            s.execute("text.preedit", &json!({"text": "새 제목"})).unwrap();
            s.execute(cancel, &json!({"text": ""})).unwrap();
            assert!(Arc::ptr_eq(&doc, &s.doc().unwrap().doc));
            assert_eq!(text(s.doc().unwrap()), "기존 제목");
        }
    }

    #[test]
    fn late_commit_cannot_modify_a_new_selection() {
        let mut s = editing("제목");
        s.execute("text.preedit", &json!({"text": "안"})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        s.execute("text.commit", &json!({"text": "안"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "제목");
    }

    #[test]
    fn selected_shape_and_cjk_commits_work_without_preedit() {
        let mut s = editing("old");
        s.execute("text.exit", &json!({})).unwrap();
        s.execute("text.preedit", &json!({"text": "한글"})).unwrap();
        assert_eq!(text(&s.composition().unwrap().preview), "한글");
        s.execute("text.commit", &json!({"text": "한글"})).unwrap();
        s.execute("text.commit", &json!({"text": " 日本語 中文"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "한글 日本語 中文");
    }
}
