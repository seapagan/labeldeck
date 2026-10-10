use super::*;
use crate::edit::ui::{Message, drive_session};

fn save_host_with_check_error(
    root: &std::path::Path,
) -> crate::edit::session::SaveHost {
    std::fs::write(root.join("blocked"), "file").unwrap();
    crate::edit::session::SaveHost {
        local: root.join("labels.json"),
        config_dir: root.join("blocked/config"),
        protected: Some(root.join("out.json")),
    }
}

#[test]
fn normal_redraw_never_checks_save_or_erases_form_error_or_saved_notice() {
    use crate::edit::session::{SaveRequest, SaveTarget};
    for saved in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let host = save_host_with_check_error(dir.path());
        let mut ui = interactive_state(SessionKind::Export);
        host.refresh(&mut ui);
        assert!(matches!(ui.message, Some(Message::Error(_))));
        if saved {
            host.service(&mut ui, SaveRequest::Save(SaveTarget::Local));
        } else {
            ui.mode = Mode::Edit(EditForm::new(
                None,
                Draft {
                    name: String::new(),
                    color: "ededed".into(),
                    description: String::new(),
                },
            ));
            key(&mut ui, KeyCode::Enter, KeyModifiers::NONE);
        }
        let before = match &ui.message {
            Some(Message::Error(text) | Message::Status(text)) => text.clone(),
            None => panic!("expected message"),
        };
        let mut events = [
            Event::Resize(100, 30),
            Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
            )),
        ]
        .into_iter();
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        drive_session(
            &mut terminal,
            &mut ui,
            || Ok(events.next().unwrap()),
            |_, _| panic!("normal redraw requested Save service"),
        )
        .unwrap();
        assert!(
            matches!(&ui.message, Some(Message::Error(text) | Message::Status(text)) if text == &before)
        );
        assert_eq!(ui.saves.len(), usize::from(saved));
    }
}

#[cfg(unix)]
#[test]
fn save_modal_refreshes_once_per_open_after_external_alias_change() {
    use crate::edit::session::{SaveHost, SaveRequest};
    let dir = tempfile::tempdir().unwrap();
    let protected = dir.path().join("out.json");
    std::fs::write(&protected, "protected").unwrap();
    let host = SaveHost {
        local: dir.path().join("labels.json"),
        config_dir: dir.path().join("config"),
        protected: Some(protected.clone()),
    };
    let mut ui = interactive_state(SessionKind::Export);
    let mut events = [
        KeyCode::Char('s'),
        KeyCode::Tab,
        KeyCode::Esc,
        KeyCode::Char('s'),
        KeyCode::Esc,
        KeyCode::Char('q'),
        KeyCode::Char('q'),
    ]
    .into_iter();
    let mut opens = 0;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    drive_session(
        &mut terminal,
        &mut ui,
        || {
            Ok(Event::Key(KeyEvent::new(
                events.next().unwrap(),
                KeyModifiers::NONE,
            )))
        },
        |ui, request| {
            assert_eq!(request, SaveRequest::Refresh);
            opens += 1;
            if opens == 2 {
                std::os::unix::fs::symlink(&protected, &host.local).unwrap();
            }
            host.service(ui, request);
            assert_eq!(ui.save_choices[0], opens == 1);
            assert!(ui.save_paths[0].contains(if opens == 1 {
                "new file"
            } else {
                "overwrite"
            }));
        },
    )
    .unwrap();
    assert_eq!(opens, 2);
    assert_eq!(std::fs::read_to_string(protected).unwrap(), "protected");
}
