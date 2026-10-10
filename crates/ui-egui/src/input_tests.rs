use super::*;

fn editor() -> SlideApp {
    let mut session = Session::with_new();
    let id = session.doc().unwrap().current_slide().unwrap().shapes[0].id;
    session.execute("text.set", &json!({"id": id, "text": ""})).unwrap();
    session.execute("text.edit", &json!({"id": id, "at": [0, 0]})).unwrap();
    SlideApp::new(session, Services::default())
}

fn frame(ctx: &egui::Context, app: &mut SlideApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput { events, screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 720.0))), ..Default::default() },
        |ui| {
            app.keyboard(ui.ctx());
            canvas::show(app, ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn preedit(text: &str) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Preedit { text: text.into(), active_range_chars: None })
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }
}

fn shortcut(key: egui::Key, ctrl: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { command: !ctrl, ctrl, ..Default::default() },
    }
}

fn commit(text: &str) -> egui::Event {
    egui::Event::Ime(egui::ImeEvent::Commit(text.into()))
}

fn text(app: &mut SlideApp) -> String {
    app.session.execute("text.get", &json!({})).unwrap()["text"].as_str().unwrap().to_owned()
}

#[test]
fn idle_empty_preedit_does_not_swallow_editing_keys() {
    for (event, expected, caret) in [(key(egui::Key::Backspace), "기", (0, 1)), (key(egui::Key::ArrowLeft), "기존", (0, 1))] {
        let ctx = egui::Context::default();
        let mut app = editor();
        app.session.type_text("기존").unwrap();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit(""), event]);
        assert_eq!(text(&mut app), expected);
        assert_eq!(app.session.doc().unwrap().selection.text.as_ref().unwrap().caret, caret);
    }
}

#[test]
fn backspace_that_ends_preedit_keeps_committed_text() {
    for dismiss in [preedit(""), commit("")] {
        let ctx = egui::Context::default();
        let mut app = editor();
        app.session.type_text("기존").unwrap();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("안")]);
        frame(&ctx, &mut app, vec![dismiss, key(egui::Key::Backspace)]);
        assert_eq!(text(&mut app), "기존");
        frame(&ctx, &mut app, vec![key(egui::Key::Backspace)]);
        assert_eq!(text(&mut app), "기");
    }
}

#[test]
fn commit_does_not_swallow_following_navigation_or_deletion() {
    for (event, expected, caret) in [(key(egui::Key::ArrowLeft), "안", (0, 0)), (key(egui::Key::Backspace), "", (0, 0))] {
        let ctx = egui::Context::default();
        let mut app = editor();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("안")]);
        frame(&ctx, &mut app, vec![preedit(""), commit("안"), event]);
        assert_eq!(text(&mut app), expected);
        assert_eq!(app.session.doc().unwrap().selection.text.as_ref().unwrap().caret, caret);
    }
}

#[test]
fn commit_only_confirmation_enter_is_suppressed_in_either_order() {
    for events in [vec![key(egui::Key::Enter), commit("日本語")], vec![commit("日本語"), key(egui::Key::Enter)]] {
        let ctx = egui::Context::default();
        let mut app = editor();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, events);
        assert_eq!(text(&mut app), "日本語");
    }
}

#[test]
fn independent_typing_after_commit_keeps_enter() {
    let ctx = egui::Context::default();
    let mut app = editor();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![commit("안"), egui::Event::Text("!".into()), key(egui::Key::Enter)]);
    assert_eq!(text(&mut app), "안!\n");
}

#[test]
fn ignored_ime_line_break_does_not_swallow_enter() {
    for newline in ["\n", "\r"] {
        for event in [preedit(newline), commit(newline)] {
            let ctx = egui::Context::default();
            let mut app = editor();
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![event, key(egui::Key::Enter)]);
            assert_eq!(text(&mut app), "\n");
        }
    }
}

#[test]
fn dismissed_preedit_allows_fresh_commit_only_input_after_navigation() {
    for next in ["日本語", "é", "안"] {
        let ctx = egui::Context::default();
        let mut app = editor();
        app.session.type_text("제목").unwrap();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("안"), preedit("")]);
        frame(&ctx, &mut app, vec![key(egui::Key::ArrowLeft)]);
        frame(&ctx, &mut app, vec![commit(next)]);
        assert_eq!(text(&mut app), format!("제{next}목"));
    }
}

#[test]
fn unbound_character_key_does_not_finish_native_composition() {
    let ctx = egui::Context::default();
    let mut app = editor();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![preedit("ㅇ")]);
    frame(&ctx, &mut app, vec![key(egui::Key::S), preedit("안")]);
    assert_eq!(text(&mut app), "");
    assert_eq!(app.session.composition().unwrap().text(), "안");
    frame(&ctx, &mut app, vec![commit("안")]);
    assert_eq!(text(&mut app), "안");
}

#[test]
fn clipboard_consuming_queued_commit_keeps_confirmation_enter_out() {
    let ctx = egui::Context::default();
    let mut app = editor();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![preedit("안")]);
    frame(&ctx, &mut app, vec![egui::Event::Copy, preedit(""), commit("안"), key(egui::Key::Enter)]);
    assert_eq!(text(&mut app), "안");
}

#[test]
fn save_shortcut_preserves_pending_syllable_in_reopened_file() {
    for ctrl in [false, true] {
        let ctx = egui::Context::default();
        let mut app = editor();
        app.session.type_text("안녕하세").unwrap();
        let path = std::env::temp_dir().join(format!("deckcraft-ime-save-{}-{ctrl}.deckcraft", std::process::id()));
        app.session.doc_mut().unwrap().path = Some(path.to_string_lossy().into_owned());
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("요")]);
        frame(&ctx, &mut app, vec![shortcut(egui::Key::S, ctrl)]);
        frame(&ctx, &mut app, vec![preedit(""), commit("요")]);
        assert_eq!(text(&mut app), "안녕하세요");
        assert!(!app.session.doc().unwrap().is_dirty());
        let mut reopened = Session::new();
        reopened.execute("file.open", &json!({"path": path})).unwrap();
        assert_eq!(reopened.doc().unwrap().current_slide().unwrap().shapes[0].text.as_ref().unwrap().text(), "안녕하세요");
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn bold_finishes_composition_once_and_next_identical_composition_still_types() {
    let ctx = egui::Context::default();
    let mut app = editor();
    app.session.type_text("안녕하세").unwrap();
    frame(&ctx, &mut app, vec![]);
    let undo = app.session.doc().unwrap().history.undo.len();
    frame(&ctx, &mut app, vec![preedit("요")]);
    frame(&ctx, &mut app, vec![shortcut(egui::Key::B, false)]);
    let journal = app.session.journal.len();
    frame(&ctx, &mut app, vec![preedit(""), commit("요")]);
    assert_eq!(text(&mut app), "안녕하세요");
    assert_eq!(app.session.journal.len(), journal);
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 2);
    frame(&ctx, &mut app, vec![preedit("요"), commit("요")]);
    assert_eq!(text(&mut app), "안녕하세요요");
    app.session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(text(&mut app), "안녕하세요");
    let mut replay = Session::new();
    for (id, params) in &app.session.journal {
        replay.execute(id, params).unwrap();
    }
    assert_eq!(replay.doc().unwrap().doc, app.session.doc().unwrap().doc);
    assert_eq!(replay.doc().unwrap().history.undo.len(), app.session.doc().unwrap().history.undo.len());
}

#[test]
fn shortcut_prefers_queued_conversion_commit_and_does_not_reject_next_commit() {
    for events in [vec![shortcut(egui::Key::B, false), commit("测试")], vec![preedit(""), shortcut(egui::Key::B, false), commit("测试")]] {
        let ctx = egui::Context::default();
        let mut app = editor();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("ce")]);
        frame(&ctx, &mut app, events);
        assert_eq!(text(&mut app), "测试");
        frame(&ctx, &mut app, vec![commit("测试")]);
        assert_eq!(text(&mut app), "测试测试");
    }
}

#[test]
fn app_shortcuts_and_clipboard_actions_preserve_composition() {
    for event in [key(egui::Key::F5), egui::Event::Paste("!".into())] {
        let ctx = egui::Context::default();
        let mut app = editor();
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![preedit("안")]);
        frame(&ctx, &mut app, vec![event]);
        assert!(text(&mut app).starts_with("안"));
    }
}

#[test]
fn canvas_enables_ime_and_keeps_korean_confirmation_keys_out_of_the_document() {
    let ctx = egui::Context::default();
    let mut app = editor();
    let out = frame(&ctx, &mut app, vec![]);
    let initial = out.platform_output.ime.expect("canvas must enable native IME");
    assert!(initial.cursor_rect.is_finite());
    let original = app.session.doc().unwrap().doc.clone();
    let undo = app.session.doc().unwrap().history.undo.len();
    frame(&ctx, &mut app, vec![preedit("ㅇ")]);
    let out = frame(&ctx, &mut app, vec![preedit("안녕하세요")]);
    assert!(std::sync::Arc::ptr_eq(&original, &app.session.doc().unwrap().doc));
    assert!(out.platform_output.ime.unwrap().cursor_rect.min.x > initial.cursor_rect.min.x);
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Commit("안녕하세요".into())),
            egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() },
        ],
    );
    assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], "안녕하세요");
    assert_eq!(app.session.doc().unwrap().history.undo.len(), undo + 1);
    frame(&ctx, &mut app, vec![egui::Event::Text("!".into())]);
    assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], "안녕하세요!");
}

#[test]
fn focused_notes_field_owns_ime_events_exclusively() {
    let ctx = egui::Context::default();
    let mut app = editor();
    app.session.execute("text.preedit", &json!({"text": "중"})).unwrap();
    let id = egui::Id::new("notes_test");
    let mut notes = String::new();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        ui.add(egui::TextEdit::singleline(&mut notes).id(id)).request_focus();
    });
    output.textures_delta.clear();
    for event in [preedit("안"), egui::Event::Ime(egui::ImeEvent::Commit("안녕하세요".into()))] {
        let mut output = ctx.run_ui(egui::RawInput { events: vec![event], ..Default::default() }, |ui| {
            app.keyboard(ui.ctx());
            ui.add(egui::TextEdit::singleline(&mut notes).id(id));
        });
        output.textures_delta.clear();
        assert!(
            !output.platform_output.ime.unwrap().should_interrupt_composition,
            "canvas cancellation must not interrupt the notes field's new IME session"
        );
    }
    assert_eq!(notes, "안녕하세요");
    assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], "");
    assert!(app.session.composition().is_none());
}

#[test]
fn canvas_interrupts_composition_after_target_changes_and_rejects_late_commit() {
    let ctx = egui::Context::default();
    let mut app = editor();
    app.session.type_text("제목").unwrap();
    app.session.execute("text.move", &json!({"to": "start"})).unwrap();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![preedit("안")]);
    app.session.execute("text.move", &json!({"to": "end"})).unwrap();
    let out = frame(&ctx, &mut app, vec![preedit("안녕")]);
    assert!(out.platform_output.ime.unwrap().should_interrupt_composition);
    let out = frame(&ctx, &mut app, vec![preedit(""), egui::Event::Ime(egui::ImeEvent::Commit("안녕".into()))]);
    assert!(!out.platform_output.ime.unwrap().should_interrupt_composition);
    assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], "제목");
    frame(&ctx, &mut app, vec![preedit("새"), preedit(""), egui::Event::Ime(egui::ImeEvent::Commit("새".into()))]);
    assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], "제목새");
}

#[test]
fn mobile_ime_can_replace_committed_jamo_and_suggestions() {
    let ctx = egui::Context::default();
    let mut app = editor();
    frame(&ctx, &mut app, vec![]);
    for (original, count, replacement) in [("ㅇ", 1, "아"), ("tex", 3, "Texas")] {
        app.session.execute("text.selectAll", &json!({})).unwrap();
        frame(&ctx, &mut app, vec![egui::Event::Text(original.into())]);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Ime(egui::ImeEvent::DeleteSurrounding { before_chars: count, after_chars: 0 }), egui::Event::Text(replacement.into())],
        );
        assert_eq!(app.session.execute("text.get", &json!({})).unwrap()["text"], replacement);
    }
}

#[test]
fn complete_editor_keeps_canvas_ime_focus_across_preedit_frames() {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut app = editor();
    app.fonts_ready = true;
    app.ui.notes = true;
    for text in ["", "", "ㅇ", "아", "안", "안녕", "안녕하세요"] {
        let events = if text.is_empty() { vec![] } else { vec![preedit(text)] };
        let mut out = ctx.run_ui(
            egui::RawInput {
                events,
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        out.textures_delta.clear();
        assert!(
            out.platform_output.ime.is_some(),
            "IME disabled at {text:?}; focused {:?}, canvas {:?}",
            ctx.memory(|m| m.focused()),
            app.canvas_input_id
        );
        if !text.is_empty() {
            assert!(!out.platform_output.ime.unwrap().should_interrupt_composition, "IME interrupted at {text:?}");
            assert!(app.session.composition().is_some(), "preedit lost at {text:?}");
        }
    }
}
