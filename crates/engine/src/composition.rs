//! IME preedit lives outside the document. A commit inserts it once; surrounding deletions
//! edit committed text while keeping the pending composition on the same target.

use std::sync::Arc;

use deckcraft_model::{ShapeKind, edit as ed};
use serde_json::{Value, json};

use crate::{DocState, Result, Selection, Session, TextSel};

#[derive(Clone, Debug)]
pub struct Composition {
    uid: u64,
    revision: u64,
    selection: Selection,
    text: String,
    preview_visible: bool,
    cancelled: bool,
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
        self.composition.as_ref().filter(|c| !c.cancelled && c.preview_visible && self.active().is_some_and(|st| c.matches(st)))
    }

    /// The canvas must interrupt the native IME before accepting composition on a new target.
    /// An explicit cancellation acknowledges that reset, so it only needs to happen once.
    pub fn composition_target_changed(&self) -> bool {
        self.composition.as_ref().is_some_and(|c| !c.cancelled && self.active().is_none_or(|st| !c.matches(st)))
    }

    pub(crate) fn cancel_composition(&mut self) {
        if let Some(c) = &mut self.composition {
            c.cancelled = true;
            c.preview_visible = false;
        }
    }

    /// Direct text input starts a fresh typing session after native composition was cancelled.
    pub(crate) fn clear_cancelled_composition(&mut self) {
        if self.composition.as_ref().is_some_and(|c| c.cancelled) {
            self.composition = None;
        }
    }

    pub(crate) fn preedit_text(&mut self, text: &str) -> Result<Value> {
        if text.is_empty() {
            // winit sends an empty preedit immediately before Commit. Keep its target so
            // a commit arriving after a selection change cannot edit the new selection.
            if let Some(c) = &mut self.composition {
                c.preview_visible = false;
            }
            return Ok(json!({"handled": true}));
        }
        // Input methods only need short strings; reject unbounded control/MCP input.
        if text.len() > 1 << 20 {
            return Err(crate::cmd::bad("text.preedit", "composition exceeds 1 MiB"));
        }
        let st = self.doc()?;
        if self.composition_target_changed() {
            return Ok(json!({"handled": false}));
        }
        let Some(mut ts) = text_target(st) else { return Ok(json!({"handled": false})) };
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
        self.composition = Some(Composition {
            uid: st.uid,
            revision: st.revision,
            selection: st.selection.clone(),
            text: text.to_string(),
            preview_visible: true,
            cancelled: false,
            preview,
            start: at,
            end,
        });
        Ok(json!({"handled": true}))
    }

    pub(crate) fn commit_text(&mut self, text: &str) -> Result<Value> {
        if text.len() > 1 << 20 {
            return Err(crate::cmd::bad("text.commit", "composition exceeds 1 MiB"));
        }
        let stale = self.composition.as_ref().is_some_and(|c| c.cancelled || self.active().is_none_or(|st| !c.matches(st)));
        self.composition = None;
        if stale || text.is_empty() || matches!(text, "\n" | "\r") {
            return Ok(json!({"handled": false}));
        }
        self.type_text(text)
    }

    /// Delete committed characters around the selected or composing range, keeping that
    /// range intact. Counts are Unicode scalars and clamp at the text body's boundaries.
    pub(crate) fn delete_surrounding_text(&mut self, before: usize, after: usize) -> Result<Value> {
        if self.composition.as_ref().is_some_and(|c| c.cancelled) || self.composition_target_changed() {
            return Ok(json!({"handled": false}));
        }
        let pending = self.composition.as_ref().filter(|c| !c.cancelled).map(|c| (c.text.clone(), c.preview_visible));
        let st = self.doc()?;
        let Some(mut ts) = text_target(st) else { return Ok(json!({"handled": false})) };
        let Some(body) = crate::cmd::text::body_of(st, &ts) else { return Ok(json!({"handled": false})) };
        let anchor = offset_of(body, ts.anchor);
        let caret = offset_of(body, ts.caret);
        let start = anchor.min(caret);
        let end = anchor.max(caret);
        let from = start.saturating_sub(before);
        let to = end.saturating_add(after).min(offset_of(body, ed::end(body)));
        if from == start && to == end {
            return Ok(json!({"handled": true}));
        }
        self.edit(|doc, sel| {
            let body = crate::cmd::text::body_mut(doc, sel, &ts)
                .ok_or_else(|| crate::cmd::bad("text.deleteSurrounding", "the text being edited is gone"))?;
            let after_start = position_at(body, end);
            let after_end = position_at(body, to);
            ed::delete(body, after_start, after_end);
            let before_start = position_at(body, from);
            let before_end = position_at(body, start);
            ed::delete(body, before_start, before_end);
            let removed = start - from;
            ts.anchor = position_at(body, anchor - removed);
            ts.caret = position_at(body, caret - removed);
            sel.text = Some(ts.clone());
            Ok(())
        })?;
        if !ts.notes && ts.cell.is_none() {
            crate::cmd::text::fit_text_box(self, ts.shape)?;
        }
        if let Some((text, visible)) = pending {
            // This is an IME edit on the same target; refresh the snapshot instead of treating
            // its changed revision/selection as an unrelated edit.
            self.composition = None;
            self.preedit_text(&text)?;
            if let Some(c) = &mut self.composition {
                c.preview_visible = visible;
            }
        }
        Ok(json!({"handled": true}))
    }
}

fn text_target(st: &DocState) -> Option<TextSel> {
    if let Some(ts) = &st.selection.text {
        return Some(ts.clone());
    }
    let id = st.selection.shapes.first().copied().filter(|_| st.selection.shapes.len() == 1)?;
    let sh = st.shape(id).filter(|sh| matches!(sh.kind, ShapeKind::Shape | ShapeKind::Connector { .. }))?;
    // Like type_text: typing on a selected shape replaces its whole text.
    Some(TextSel { shape: id, caret: sh.text.as_ref().map(ed::end).unwrap_or((0, 0)), ..Default::default() })
}

fn offset_of(body: &deckcraft_model::TextBody, pos: ed::Pos) -> usize {
    let para = pos.0.min(body.paragraphs.len().saturating_sub(1));
    let prefix = body.paragraphs.iter().take(para).fold(0usize, |n, p| n.saturating_add(p.char_len()).saturating_add(1));
    prefix.saturating_add(body.paragraphs.get(para).map(|p| pos.1.min(p.char_len())).unwrap_or(0))
}

fn position_at(body: &deckcraft_model::TextBody, mut offset: usize) -> ed::Pos {
    for (pi, p) in body.paragraphs.iter().enumerate() {
        let len = p.char_len();
        if offset <= len {
            return (pi, offset);
        }
        offset = offset.saturating_sub(len).saturating_sub(1);
    }
    ed::end(body)
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
    fn empty_preedit_before_late_commit_keeps_the_original_target() {
        let mut s = editing("제목");
        s.execute("text.preedit", &json!({"text": "안"})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        s.execute("text.preedit", &json!({"text": ""})).unwrap();
        assert!(s.composition().is_none());
        assert!(s.composition_target_changed());
        s.execute("text.commit", &json!({"text": "안"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "제목");
    }

    #[test]
    fn stale_preedit_needs_a_backend_reset_before_a_fresh_composition() {
        let mut s = editing("제목");
        s.execute("text.preedit", &json!({"text": "안"})).unwrap();
        s.execute("text.move", &json!({"to": "end"})).unwrap();
        let r = s.execute("text.preedit", &json!({"text": "안녕"})).unwrap();
        assert_eq!(r["handled"], false);
        assert!(s.composition_target_changed());
        s.execute("text.cancelComposition", &json!({})).unwrap();
        assert!(!s.composition_target_changed());
        s.execute("text.preedit", &json!({"text": "새"})).unwrap();
        s.execute("text.preedit", &json!({"text": ""})).unwrap();
        s.execute("text.commit", &json!({"text": "새"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "제목새");
    }

    #[test]
    fn cancelled_composition_rejects_a_late_commit_after_preedit_clear() {
        let mut s = editing("제목");
        s.execute("text.preedit", &json!({"text": "안"})).unwrap();
        s.execute("text.cancelComposition", &json!({})).unwrap();
        s.execute("text.preedit", &json!({"text": ""})).unwrap();
        s.execute("text.commit", &json!({"text": "안"})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "제목");
    }

    #[test]
    fn delete_surrounding_supports_mobile_hangul_and_word_replacements() {
        for (original, replacement) in [("ㅇ", "아"), ("tex", "Texas")] {
            let mut s = editing(original);
            s.execute("text.move", &json!({"to": "end"})).unwrap();
            let undo = s.doc().unwrap().history.undo.len();
            s.execute("text.deleteSurrounding", &json!({"beforeChars": original.chars().count(), "afterChars": 0})).unwrap();
            assert_eq!(s.doc().unwrap().history.undo.len(), undo + 1);
            s.execute("text.commit", &json!({"text": replacement})).unwrap();
            assert_eq!(text(s.doc().unwrap()), replacement);
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(text(s.doc().unwrap()), "");
            s.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(text(s.doc().unwrap()), original);
        }
    }

    #[test]
    fn delete_surrounding_preserves_selected_text_and_direction() {
        let mut s = editing("앞가운데뒤");
        s.execute("text.select", &json!({"anchor": [0, 4], "caret": [0, 1]})).unwrap();
        s.execute("text.deleteSurrounding", &json!({"beforeChars": 1, "afterChars": 1})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "가운데");
        let ts = s.doc().unwrap().selection.text.as_ref().unwrap();
        assert_eq!(ts.anchor, (0, 3));
        assert_eq!(ts.caret, (0, 0));
    }

    #[test]
    fn delete_surrounding_counts_unicode_scalars_and_clamps_large_counts() {
        let mut s = editing("가🙂\n나🦀\n다");
        s.execute("text.select", &json!({"anchor": [1, 0], "caret": [1, 2]})).unwrap();
        s.execute("text.deleteSurrounding", &json!({"beforeChars": 1, "afterChars": 1})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "가🙂나🦀다");
        let ts = s.doc().unwrap().selection.text.as_ref().unwrap();
        assert_eq!(ts.anchor, (0, 2));
        assert_eq!(ts.caret, (0, 4));
        s.execute("text.deleteSurrounding", &json!({"beforeChars": u64::MAX, "afterChars": u64::MAX})).unwrap();
        assert_eq!(text(s.doc().unwrap()), "나🦀");
    }

    #[test]
    fn surrounding_deletion_refreshes_pending_composition_for_commit_or_cancel() {
        for finish in ["text.commit", "text.cancelComposition"] {
            let mut s = editing("앞기존뒤");
            s.execute("text.select", &json!({"anchor": [0, 1], "caret": [0, 3]})).unwrap();
            s.execute("text.preedit", &json!({"text": "안"})).unwrap();
            s.execute("text.deleteSurrounding", &json!({"beforeChars": 1, "afterChars": 1})).unwrap();
            assert!(!s.composition_target_changed());
            assert_eq!(text(s.doc().unwrap()), "기존");
            assert_eq!(text(&s.composition().unwrap().preview), "안");
            s.execute(finish, &json!({"text": "안"})).unwrap();
            assert_eq!(text(s.doc().unwrap()), if finish == "text.commit" { "안" } else { "기존" });
        }
    }

    #[test]
    fn cancelled_composition_rejects_late_surrounding_deletion() {
        for move_caret in [false, true] {
            let mut s = editing("제목");
            s.execute("text.preedit", &json!({"text": "안"})).unwrap();
            if move_caret {
                s.execute("text.move", &json!({"to": "end"})).unwrap();
            }
            s.execute("text.cancelComposition", &json!({})).unwrap();
            let original = s.doc().unwrap().doc.clone();
            let r = s.execute("text.deleteSurrounding", &json!({"beforeChars": 1, "afterChars": 1})).unwrap();
            assert_eq!(r["handled"], false);
            assert!(Arc::ptr_eq(&original, &s.doc().unwrap().doc));
            assert_eq!(text(s.doc().unwrap()), "제목");
        }
    }

    #[test]
    fn direct_mobile_text_starts_a_fresh_session_after_cancellation() {
        let mut s = editing("");
        s.execute("text.preedit", &json!({"text": "안"})).unwrap();
        s.execute("text.cancelComposition", &json!({})).unwrap();
        s.type_text("ㅇ").unwrap();
        s.execute("text.deleteSurrounding", &json!({"beforeChars": 1, "afterChars": 0})).unwrap();
        s.type_text("아").unwrap();
        assert_eq!(text(s.doc().unwrap()), "아");
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
