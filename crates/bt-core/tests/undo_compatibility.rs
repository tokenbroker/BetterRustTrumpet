use bt_core::undo::{Change, MAX_STACK_DEPTH, Target, UndoHistory};

fn target(id: &str) -> Target {
    Target {
        id: id.into(),
        display_name: format!("app-{id}"),
        is_device: false,
    }
}

#[test]
fn coalescing_uses_sliding_strict_500ms_window_and_original_old_value() {
    let mut history = UndoHistory::default();
    history.record_volume_change(target("a"), 10, 20, 0);
    history.record_volume_change(target("a"), 20, 30, 499);
    history.record_volume_change(target("a"), 30, 40, 998);
    assert_eq!(history.undo_count(), 1);
    assert_eq!(
        history.undo_description().as_deref(),
        Some("Undo volume app-a (10→40)")
    );
    history.record_volume_change(target("a"), 40, 50, 1498);
    assert_eq!(history.undo_count(), 2);
    assert_eq!(
        history.undo().unwrap().change,
        Change::Volume { old: 40, new: 50 }
    );
    assert_eq!(
        history.undo().unwrap().change,
        Change::Volume { old: 10, new: 40 }
    );
}

#[test]
fn fresh_changes_including_coalescing_clear_redo_but_noops_do_not() {
    let mut history = UndoHistory::default();
    history.record_volume_change(target("a"), 10, 20, 0);
    history.record_volume_change(target("b"), 10, 20, 1);
    history.undo();
    let revision = history.revision();
    assert!(!history.record_volume_change(target("a"), 20, 20, 2));
    assert!(history.can_redo());
    assert_eq!(history.revision(), revision);
    history.record_volume_change(target("a"), 20, 25, 2);
    assert!(!history.can_redo());
    assert_eq!(history.undo_count(), 1);
    assert_eq!(
        history.undo().unwrap().change,
        Change::Volume { old: 10, new: 25 }
    );
}

#[test]
fn mute_actions_are_separate_and_replay_suppresses_recording() {
    let mut history = UndoHistory::default();
    history.record_volume_change(target("a"), 10, 20, 0);
    history.record_mute_change(target("a"), false, true, 1);
    history.record_mute_change(target("a"), true, false, 2);
    assert_eq!(history.undo_count(), 3);
    assert_eq!(
        history.undo_description().as_deref(),
        Some("Undo unmute app-a")
    );
    history.undo();
    assert_eq!(
        history.redo_description().as_deref(),
        Some("Redo unmute app-a")
    );
    history.begin_undo_redo();
    assert!(!history.record_volume_change(target("a"), 20, 10, 3));
    assert!(!history.record_mute_change(target("a"), false, true, 3));
    history.end_undo_redo();
    assert_eq!(
        history.redo().unwrap().change,
        Change::Mute {
            old: true,
            new: false
        }
    );
}

#[test]
fn capacity_discards_oldest_actions_and_empty_operations_do_not_notify() {
    let mut history = UndoHistory::default();
    assert_eq!(history.undo(), None);
    assert_eq!(history.redo(), None);
    assert_eq!(history.revision(), 0);
    for n in 0..55 {
        history.record_volume_change(target(&n.to_string()), 0, n, n as i64);
    }
    assert_eq!(history.undo_count(), MAX_STACK_DEPTH);
    let mut last = None;
    while let Some(action) = history.undo() {
        last = Some(action);
    }
    assert_eq!(last.unwrap().target.id, "5");
    assert_eq!(history.redo_count(), MAX_STACK_DEPTH);
    history.clear();
    assert!(!history.can_redo());
    let revision = history.revision();
    history.clear();
    assert_eq!(history.revision(), revision + 1);
}

#[test]
fn coalescing_preserves_source_id_only_and_backwards_clock_quirks() {
    let mut history = UndoHistory::default();
    history.record_volume_change(target("a"), 0, 10, 10_000);
    let changed_target = Target {
        is_device: true,
        display_name: "different".into(),
        ..target("a")
    };
    history.record_volume_change(changed_target, 10, 15, -10_000);
    assert_eq!(history.undo_count(), 1);
    let action = history.undo().unwrap();
    assert!(!action.target.is_device);
    assert_eq!(action.target.display_name, "app-a");
    assert_eq!(action.timestamp_ms, -10_000);
}
