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
