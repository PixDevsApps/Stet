use super::*;
use proptest::collection::vec;
use proptest::prelude::*;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;

fn non_utf8_path() -> PathBuf {
    PathBuf::from(OsString::from_vec(b"/tmp/caf\xe9.txt".to_vec()))
}

fn sample() -> Session {
    Session {
        tabs: vec![
            TabRecord {
                id: record_id("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10"),
                path: Some("/home/user/notes/todo.md".into()),
                display_name: "todo.md".into(),
                dirty: true,
                backup: Some(record_id("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10")),
                encoding: Some("UTF-8".into()),
                bom: Some(false),
                eol: Some(Eol::CrLf),
                language: Some("markdown".into()),
                caret: 120,
                selection: Some((100, 120)),
                first_visible_line: 3,
                disk_fingerprint: Some(Fingerprint {
                    dev: 57,
                    ino: 10339,
                    size: 2048,
                    mtime_ns: 1_790_000_000_123_456_789,
                }),
                chosen_encoding: Some("windows-1252".into()),
                lossy: true,
                bookmarks: vec![3, 10],
                ..TabRecord::default()
            },
            TabRecord {
                id: record_id("0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2"),
                display_name: "Shopping list".into(),
                custom_name: Some("Shopping list".into()),
                untitled_number: Some(1),
                dirty: true,
                backup: Some(record_id("0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2")),
                pinned: true,
                view: ViewSlot::Second,
                ..TabRecord::default()
            },
            TabRecord {
                id: record_id("0199a3b2-7c20-7f00-9b11-0a1b2c3d4e5f"),
                display_name: "todo.md".into(),
                caret: 4,
                view: ViewSlot::Second,
                clone_of: Some(record_id("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10")),
                ..TabRecord::default()
            },
        ],
        active_tab: Some(1),
        other_tab: Some(0),
        window: WindowState {
            width: 1400,
            height: 900,
            maximized: false,
            zoom: 2,
            split_position: 700,
        },
        recent_files: vec!["/home/user/notes/todo.md".into(), non_utf8_path()],
        find_history: vec!["TODO".into()],
        replace_history: vec!["DONE".into()],
        directory_history: vec!["~/src".into()],
        filter_history: vec!["*.rs !target/".into()],
        ..Session::default()
    }
}

fn record_id(text: &str) -> RecordId {
    text.parse().unwrap()
}

/// A tab with `id`, read from `session.json` as any id there is.
fn tab_with_id(id: &str) -> TabRecord {
    serde_json::from_value(json!({ "id": id })).unwrap()
}

fn ids(session: &Session) -> Vec<&str> {
    session.tabs.iter().map(|tab| tab.id.as_str()).collect()
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "an inline snapshot of a whole session.json"
)]
fn json_format() {
    insta::assert_snapshot!(serde_json::to_string_pretty(&sample()).unwrap(), @r#"
    {
      "schema_version": 1,
      "tabs": [
        {
          "id": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
          "path": "/home/user/notes/todo.md",
          "display_name": "todo.md",
          "untitled_number": null,
          "dirty": true,
          "backup": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
          "read_only": false,
          "encoding": "UTF-8",
          "bom": false,
          "eol": "CRLF",
          "language": "markdown",
          "caret": 120,
          "selection": [
            100,
            120
          ],
          "first_visible_line": 3,
          "pinned": false,
          "view": 0,
          "clone_of": null,
          "bookmarks": [
            3,
            10
          ],
          "disk_fingerprint": {
            "dev": 57,
            "ino": 10339,
            "size": 2048,
            "mtime_ns": 1790000000123456789
          },
          "chosen_encoding": "windows-1252",
          "lossy": true
        },
        {
          "id": "0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2",
          "path": null,
          "display_name": "Shopping list",
          "custom_name": "Shopping list",
          "untitled_number": 1,
          "dirty": true,
          "backup": "0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2",
          "read_only": false,
          "encoding": null,
          "bom": null,
          "eol": null,
          "language": null,
          "caret": 0,
          "selection": null,
          "first_visible_line": 0,
          "pinned": true,
          "view": 1,
          "clone_of": null,
          "bookmarks": [],
          "disk_fingerprint": null,
          "chosen_encoding": null,
          "lossy": false
        },
        {
          "id": "0199a3b2-7c20-7f00-9b11-0a1b2c3d4e5f",
          "path": null,
          "display_name": "todo.md",
          "untitled_number": null,
          "dirty": false,
          "backup": null,
          "read_only": false,
          "encoding": null,
          "bom": null,
          "eol": null,
          "language": null,
          "caret": 4,
          "selection": null,
          "first_visible_line": 0,
          "pinned": false,
          "view": 1,
          "clone_of": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
          "bookmarks": [],
          "disk_fingerprint": null,
          "chosen_encoding": null,
          "lossy": false
        }
      ],
      "active_tab": 1,
      "window": {
        "width": 1400,
        "height": 900,
        "maximized": false,
        "zoom": 2,
        "split_position": 700
      },
      "recent_files": [
        "/home/user/notes/todo.md",
        {
          "bytes": [
            47,
            116,
            109,
            112,
            47,
            99,
            97,
            102,
            233,
            46,
            116,
            120,
            116
          ]
        }
      ],
      "find_history": [
        "TODO"
      ],
      "replace_history": [
        "DONE"
      ],
      "directory_history": [
        "~/src"
      ],
      "filter_history": [
        "*.rs !target/"
      ],
      "other_tab": 0
    }
    "#);
}

#[test]
fn an_empty_object_is_the_default_session() {
    let session: Session = serde_json::from_str("{}").unwrap();
    assert_eq!(session, Session::default());
    assert_eq!(session.schema_version, SCHEMA_VERSION);
}

#[test]
fn rejects_malformed_documents() {
    for document in ["null", "\"session\"", r#"{"schema_version": "1"}"#, "{"] {
        assert!(
            serde_json::from_str::<Session>(document).is_err(),
            "{document}"
        );
    }
}

#[test]
fn a_newer_version_still_loads() {
    let session: Session = serde_json::from_value(json!({
        "schema_version": 7,
        "tabs": [{"id": "a", "future_field": {"x": 1}}],
        "future_section": [1, 2, 3],
    }))
    .unwrap();
    assert_eq!(session.schema_version, 7);
    assert_eq!(ids(&session), ["a"]);
}

#[test]
fn a_wrong_type_falls_back_per_field() {
    let original = serde_json::to_value(sample()).unwrap();
    let invalid = json!({"unexpected": [1, "x"]});
    let tab_defaults = serde_json::to_value(TabRecord::default()).unwrap();
    for key in original["tabs"][0].as_object().unwrap().keys() {
        let mut broken = original.clone();
        broken["tabs"][0][key] = invalid.clone();
        let mut expected = original.clone();
        expected["tabs"][0][key] = tab_defaults[key].clone();
        assert_eq!(
            serde_json::from_value::<Session>(broken).unwrap(),
            serde_json::from_value::<Session>(expected).unwrap(),
            "tab field {key}"
        );
    }
    let session_defaults = serde_json::to_value(Session::default()).unwrap();
    for key in original.as_object().unwrap().keys() {
        if key == "schema_version" {
            continue;
        }
        let mut broken = original.clone();
        broken[key] = invalid.clone();
        let mut expected = original.clone();
        expected[key] = session_defaults[key].clone();
        assert_eq!(
            serde_json::from_value::<Session>(broken).unwrap(),
            serde_json::from_value::<Session>(expected).unwrap(),
            "session field {key}"
        );
    }
}

#[test]
fn list_elements_of_another_shape_are_skipped() {
    let session: Session = serde_json::from_value(json!({
        "tabs": [{"id": "a"}, 42, "tab", {"id": "b"}],
        "recent_files": ["/a", 7, {"bytes": [47, 98]}, null],
        "find_history": ["x", 1, "y"],
        "replace_history": "not a list",
    }))
    .unwrap();
    assert_eq!(ids(&session), ["a", "b"]);
    assert_eq!(
        session.recent_files,
        [PathBuf::from("/a"), PathBuf::from("/b")]
    );
    assert_eq!(session.find_history, ["x", "y"]);
    assert!(session.replace_history.is_empty());
}

#[test]
fn line_endings_are_written_as_labels() {
    for eol in Eol::ALL {
        let value = serde_json::to_value(TabRecord {
            eol: Some(eol),
            ..TabRecord::default()
        })
        .unwrap();
        assert_eq!(value["eol"], eol.label());
        let tab: TabRecord = serde_json::from_value(value).unwrap();
        assert_eq!(tab.eol, Some(eol));
    }
    let tab: TabRecord = serde_json::from_value(json!({"eol": "NEL"})).unwrap();
    assert_eq!(tab.eol, None);
}

#[test]
fn non_utf8_paths_are_written_as_bytes() {
    let value = serde_json::to_value(TabRecord {
        path: Some(non_utf8_path()),
        ..TabRecord::default()
    })
    .unwrap();
    assert_eq!(
        value["path"],
        json!({"bytes": b"/tmp/caf\xe9.txt".to_vec()})
    );
    let tab: TabRecord = serde_json::from_value(value).unwrap();
    assert_eq!(tab.path, Some(non_utf8_path()));
}

#[test]
fn a_partial_fingerprint_reads_as_none() {
    let tab: TabRecord = serde_json::from_value(json!({
        "disk_fingerprint": {"dev": 1, "ino": 2, "size": 3},
    }))
    .unwrap();
    assert_eq!(tab.disk_fingerprint, None);
}

#[test]
fn fingerprints_ignore_the_device_number() {
    let saved = Fingerprint {
        dev: 57,
        ino: 10,
        size: 5,
        mtime_ns: 1,
    };
    assert!(saved.matches(&Fingerprint { dev: 58, ..saved }));
    assert!(!saved.matches(&Fingerprint { ino: 11, ..saved }));
    assert!(!saved.matches(&Fingerprint { size: 6, ..saved }));
    assert!(!saved.matches(&Fingerprint {
        mtime_ns: 2,
        ..saved
    }));
}

#[test]
fn ids_are_plain_file_names() {
    assert!(is_valid_id("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10"));
    assert!(is_valid_id("tab_1"));
    assert!(is_valid_id(&"x".repeat(MAX_ID_LEN)));
    let too_long = "x".repeat(MAX_ID_LEN + 1);
    for id in ["", ".", "..", "a/b", "../x", "a.txt", "a b", "é", &too_long] {
        assert!(!is_valid_id(id), "{id}");
    }
}

#[test]
fn invalid_ids_read_as_the_empty_id() {
    let tab: TabRecord = serde_json::from_value(json!({
        "id": "../x",
        "backup": "a b",
        "clone_of": "",
    }))
    .unwrap();
    assert!(tab.id.is_empty());
    assert_eq!(tab.backup, Some(RecordId::default()));
    assert_eq!(tab.clone_of, Some(RecordId::default()));
}

#[test]
fn views_are_written_as_numbers() {
    for (number, slot) in [
        (0, ViewSlot::Main),
        (1, ViewSlot::Second),
        (7, ViewSlot::Second),
    ] {
        let tab: TabRecord = serde_json::from_value(json!({ "view": number })).unwrap();
        assert_eq!(tab.view, slot, "{number}");
    }
    let tab: TabRecord = serde_json::from_value(json!({ "view": -1 })).unwrap();
    assert_eq!(tab.view, ViewSlot::Main);
    let second = TabRecord {
        view: ViewSlot::Second,
        ..TabRecord::default()
    };
    assert_eq!(serde_json::to_value(second).unwrap()["view"], 1);
}

#[test]
fn repair_replaces_invalid_and_duplicate_ids() {
    let mut session = Session {
        tabs: ["a", "", "a", "../x", "b"].map(tab_with_id).to_vec(),
        active_tab: Some(5),
        ..Session::default()
    };
    let mut counter = 0;
    session.repair(|| {
        counter += 1;
        record_id(&format!("new-{counter}"))
    });
    assert_eq!(ids(&session), ["a", "new-1", "new-2", "new-3", "b"]);
    assert_eq!(session.active_tab, None);

    session.other_tab = Some(9);
    session.repair(|| unreachable!("all ids are valid and unique"));
    assert_eq!(session.other_tab, None);

    session.active_tab = Some(4);
    session.repair(|| unreachable!("all ids are valid and unique"));
    assert_eq!(session.active_tab, Some(4));
}

#[test]
fn forgetting_drafts_keeps_file_tabs_clean() {
    let mut session = sample();
    session.tabs.push(TabRecord {
        path: Some("/c".into()),
        ..tab_with_id("c")
    });
    session.tabs.push(TabRecord {
        clone_of: Some(session.tabs[1].id.clone()),
        ..tab_with_id("untitled-clone")
    });
    session.active_tab = Some(3);
    session.other_tab = Some(2);
    let forgotten = session.without_drafts();
    // The untitled tab goes, and its clone with it; the file's clone stays.
    assert_eq!(
        ids(&forgotten),
        [
            session.tabs[0].id.as_str(),
            session.tabs[2].id.as_str(),
            "c"
        ]
    );
    assert!(
        forgotten
            .tabs
            .iter()
            .all(|tab| !tab.dirty && tab.backup.is_none())
    );
    assert_eq!(forgotten.tabs[0].caret, session.tabs[0].caret);
    assert_eq!(forgotten.active_tab, Some(2));
    assert_eq!(forgotten.other_tab, Some(1));
    assert_eq!(forgotten.recent_files, session.recent_files);
    assert_eq!(forgotten.find_history, session.find_history);

    assert_eq!(sample().without_drafts().active_tab, None);
}

#[test]
fn selections_keep_their_direction() {
    let tab = |caret, selection| TabRecord {
        caret,
        selection,
        ..TabRecord::default()
    };
    assert_eq!(tab(5, None).anchor_and_caret(10), (5, 5));
    assert_eq!(tab(50, None).anchor_and_caret(10), (10, 10));
    assert_eq!(tab(8, Some((2, 8))).anchor_and_caret(10), (2, 8));
    assert_eq!(tab(2, Some((2, 8))).anchor_and_caret(10), (8, 2));
    assert_eq!(tab(2, Some((8, 2))).anchor_and_caret(10), (8, 2));
    // Past the end of a text that a crash left shorter.
    assert_eq!(tab(30, Some((20, 30))).anchor_and_caret(25), (20, 25));
    assert_eq!(tab(20, Some((20, 30))).anchor_and_caret(15), (15, 15));
}

fn backup(chars: usize) -> BackupState {
    BackupState {
        needs_backup: true,
        changed: true,
        large_file: false,
        chars,
        since_backup: None,
        since_edit: Duration::ZERO,
    }
}

#[test]
fn changed_texts_are_backed_up_and_clean_ones_removed() {
    assert_eq!(backup(10).action(false), BackupAction::Write);
    let unchanged = BackupState {
        changed: false,
        ..backup(10)
    };
    assert_eq!(unchanged.action(false), BackupAction::Keep);
    assert_eq!(unchanged.action(true), BackupAction::Keep);
    let saved = BackupState {
        needs_backup: false,
        ..backup(10)
    };
    assert_eq!(saved.action(false), BackupAction::Remove);
    assert_eq!(saved.action(true), BackupAction::Remove);
}

#[test]
fn large_texts_back_off_and_large_files_wait_for_the_last_commit() {
    let large = LARGE_BACKUP_CHARS + 1;
    assert_eq!(
        backup(LARGE_BACKUP_CHARS).action(false),
        BackupAction::Write
    );
    // Being typed in: wait until the text is left alone.
    assert_eq!(backup(large).action(false), BackupAction::Keep);
    let idle = BackupState {
        since_edit: LARGE_BACKUP_IDLE,
        ..backup(large)
    };
    assert_eq!(idle.action(false), BackupAction::Write);
    let recent = BackupState {
        since_backup: Some(LARGE_BACKUP_INTERVAL - Duration::from_secs(1)),
        ..idle
    };
    assert_eq!(recent.action(false), BackupAction::Keep);
    let due = BackupState {
        since_backup: Some(LARGE_BACKUP_INTERVAL),
        ..idle
    };
    assert_eq!(due.action(false), BackupAction::Write);
    assert_eq!(recent.action(true), BackupAction::Write);

    let large_file = BackupState {
        large_file: true,
        since_edit: Duration::from_secs(600),
        ..backup(10)
    };
    assert_eq!(large_file.action(false), BackupAction::Keep);
    assert_eq!(large_file.action(true), BackupAction::Write);
}

fn text() -> impl Strategy<Value = String> {
    ".{0,12}"
}

/// Valid ids, and the empty one.
fn any_id() -> impl Strategy<Value = RecordId> {
    "[A-Za-z0-9_-]{0,12}".prop_map(|text| text.parse().unwrap_or_default())
}

fn path() -> impl Strategy<Value = PathBuf> {
    prop_oneof![
        "(/[a-zé漢 ._-]{0,8}){1,3}".prop_map(PathBuf::from),
        vec(any::<u8>(), 0..16).prop_map(|bytes| PathBuf::from(OsString::from_vec(bytes))),
    ]
}

fn fingerprint() -> impl Strategy<Value = Fingerprint> {
    (any::<u64>(), any::<u64>(), any::<u64>(), any::<i64>()).prop_map(
        |(dev, ino, size, mtime_ns)| Fingerprint {
            dev,
            ino,
            size,
            mtime_ns,
        },
    )
}

fn tab() -> impl Strategy<Value = TabRecord> {
    let identity = (
        any_id(),
        proptest::option::of(path()),
        text(),
        any::<Option<u32>>(),
        any::<bool>(),
        proptest::option::of(any_id()),
        any::<bool>(),
        proptest::option::of(text()),
    );
    let view = (
        any::<Option<bool>>(),
        proptest::option::of(proptest::sample::select(Eol::ALL.to_vec())),
        proptest::option::of(text()),
        any::<usize>(),
        any::<Option<(usize, usize)>>(),
        any::<usize>(),
        any::<bool>(),
        proptest::option::of(fingerprint()),
    );
    let reading = (
        proptest::option::of(text()),
        any::<bool>(),
        proptest::option::of(text()),
    );
    let views = (
        any::<u8>().prop_map(ViewSlot::from),
        proptest::option::of(any_id()),
        vec(any::<usize>(), 0..4),
    );
    (identity, view, reading, views).prop_map(
        |(
            (id, path, display_name, untitled_number, dirty, backup, read_only, encoding),
            (bom, eol, language, caret, selection, first_visible_line, pinned, disk_fingerprint),
            (chosen_encoding, lossy, custom_name),
            (view, clone_of, bookmarks),
        )| TabRecord {
            id,
            path,
            display_name,
            custom_name,
            untitled_number,
            dirty,
            backup,
            read_only,
            encoding,
            bom,
            eol,
            language,
            caret,
            selection,
            first_visible_line,
            pinned,
            disk_fingerprint,
            chosen_encoding,
            lossy,
            view,
            clone_of,
            bookmarks,
        },
    )
}

fn session() -> impl Strategy<Value = Session> {
    (
        any::<u32>(),
        vec(tab(), 0..6),
        any::<(Option<usize>, Option<usize>)>(),
        any::<(i32, i32, bool, i32, i32)>(),
        vec(path(), 0..4),
        vec(text(), 0..4),
        vec(text(), 0..4),
        vec(text(), 0..4),
        vec(text(), 0..4),
    )
        .prop_map(
            |(
                schema_version,
                tabs,
                (active_tab, other_tab),
                (width, height, maximized, zoom, split_position),
                recent_files,
                find_history,
                replace_history,
                directory_history,
                filter_history,
            )| Session {
                schema_version,
                tabs,
                active_tab,
                window: WindowState {
                    width,
                    height,
                    maximized,
                    zoom,
                    split_position,
                },
                recent_files,
                find_history,
                replace_history,
                directory_history,
                filter_history,
                other_tab,
            },
        )
}

fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        any::<i64>().prop_map(Value::from),
        any::<f64>().prop_map(Value::from),
        text().prop_map(Value::from),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::from),
            proptest::collection::btree_map("[a-z]{1,4}", inner, 0..4)
                .prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    })
}

/// Adds `key` to the session, its window, every tab and every fingerprint.
fn add_everywhere(document: &mut Value, key: &str, value: &Value) {
    document[key] = value.clone();
    document["window"][key] = value.clone();
    for tab in document["tabs"].as_array_mut().unwrap() {
        tab[key] = value.clone();
        if tab["disk_fingerprint"].is_object() {
            tab["disk_fingerprint"][key] = value.clone();
        }
    }
}

/// Removes the keys `mask` picks from `object`, and sets them to their defaults in
/// `expected` instead.
fn remove_keys(object: &mut Value, expected: &mut Value, defaults: &Value, mask: u64) {
    let keys: Vec<String> = object.as_object().unwrap().keys().cloned().collect();
    for (bit, key) in keys.iter().enumerate() {
        if mask & (1 << (bit % 64)) != 0 {
            object.as_object_mut().unwrap().remove(key);
            expected[key] = defaults[key].clone();
        }
    }
}

proptest! {
    #[test]
    fn sessions_round_trip(session in session()) {
        let json = serde_json::to_string(&session).unwrap();
        prop_assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), session);
    }

    #[test]
    fn unknown_fields_are_ignored(
        session in session(),
        key in "[a-z_]{1,8}",
        value in json_value(),
    ) {
        let mut document = serde_json::to_value(&session).unwrap();
        add_everywhere(&mut document, &format!("future_{key}"), &value);
        prop_assert_eq!(serde_json::from_value::<Session>(document).unwrap(), session);
    }

    #[test]
    fn missing_fields_take_their_defaults(
        session in session(),
        masks in vec(any::<u64>(), 1..8),
    ) {
        let mut document = serde_json::to_value(&session).unwrap();
        let mut expected = document.clone();
        let mut masks = masks.into_iter().cycle();
        let tab_defaults = serde_json::to_value(TabRecord::default()).unwrap();
        for index in 0..session.tabs.len() {
            remove_keys(
                &mut document["tabs"][index],
                &mut expected["tabs"][index],
                &tab_defaults,
                masks.next().unwrap(),
            );
        }
        remove_keys(
            &mut document["window"],
            &mut expected["window"],
            &serde_json::to_value(WindowState::default()).unwrap(),
            masks.next().unwrap(),
        );
        remove_keys(
            &mut document,
            &mut expected,
            &serde_json::to_value(Session::default()).unwrap(),
            masks.next().unwrap(),
        );
        let loaded = serde_json::from_value::<Session>(document).unwrap();
        prop_assert_eq!(loaded, serde_json::from_value::<Session>(expected).unwrap());
    }
}
