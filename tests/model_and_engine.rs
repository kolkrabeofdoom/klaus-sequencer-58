use klaus_sequencer_58::engine::{next_timed_index, probability_hits};
use klaus_sequencer_58::model::{Project, Step, StepState};

#[test]
fn demo_project_has_valid_adjacent_same_type_chains() {
    let project = Project::demo();
    assert_eq!(project.lanes.len(), 8);
    assert!(project.validate_chains().is_ok());
}

#[test]
fn project_round_trip_preserves_musical_state() {
    let project = Project::demo();
    let json = project.to_json().expect("serialize project");
    let restored = Project::from_json(&json).expect("deserialize project");
    assert_eq!(restored, project);
}

#[test]
fn skipped_steps_are_removed_from_the_timed_cycle() {
    let mut steps = vec![Step::rest(); 4];
    steps[1].state = StepState::Skip;
    steps[2].state = StepState::Skip;
    assert_eq!(next_timed_index(&steps, 0), 3);
    assert_eq!(next_timed_index(&steps, 3), 0);
}

#[test]
fn seeded_probability_is_reproducible() {
    let first: Vec<bool> = (0..32)
        .map(|cycle| probability_hits(58, 2, 7, cycle, 63))
        .collect();
    let second: Vec<bool> = (0..32)
        .map(|cycle| probability_hits(58, 2, 7, cycle, 63))
        .collect();
    assert_eq!(first, second);
    assert!(probability_hits(58, 0, 0, 0, 100));
    assert!(!probability_hits(58, 0, 0, 0, 0));
}

#[test]
fn chain_patterns_can_be_added_activated_and_switched() {
    let mut project = Project::demo();
    assert_eq!(project.patterns.len(), project.chains.len());
    let original_pattern = project.active_pattern_by_chain[0];
    let mut rendered = vec![vec![Step::rest(); 16]; 2];
    rendered[0][0] = Step::active_with_pitch(72);
    let rendered_pattern = project
        .add_chain_pattern(0, "ARP TAKE 02".to_owned(), rendered)
        .expect("add pattern");
    assert_eq!(project.active_pattern_by_chain[0], rendered_pattern);
    assert_eq!(project.lanes[0].steps[0].notes[0].pitch, 72);

    project
        .apply_chain_pattern(0, original_pattern)
        .expect("switch pattern");
    assert_ne!(project.lanes[0].steps[0].notes[0].pitch, 72);
}
