use klaus_sequencer_58::engine::{next_timed_index, probability_hits};
use klaus_sequencer_58::model::{
    ClipTransition, GeneratorRole, PerformanceAction, PerformanceEvent, PerformanceLoop, Project,
    SongClipSource, Step, StepState,
};

#[test]
fn demo_project_has_valid_adjacent_same_type_chains() {
    let project = Project::demo();
    assert_eq!(project.lanes.len(), 8);
    assert_eq!(project.chains.len(), 8);
    assert!(project.chains.iter().all(|chain| chain.lane_ids.len() == 1));
    assert!(project.validate_chains().is_ok());
}

#[test]
fn adjacent_same_type_rows_can_be_linked_and_split_again() {
    let mut project = Project::demo();
    assert!(project.can_link_lane_with_next(0));
    assert!(!project.can_link_lane_with_next(5));
    project
        .link_lane_with_next(0)
        .expect("link adjacent melody rows");
    assert_eq!(project.chains.len(), 7);
    assert_eq!(project.chains[0].lane_ids, vec![0, 1]);
    assert!(project.can_split_chain_for_lane(1));
    project
        .split_chain_for_lane(1)
        .expect("split linked melody rows");
    assert_eq!(project.chains.len(), 8);
    assert_eq!(project.chains[0].lane_ids, vec![0]);
    assert_eq!(project.chains[1].lane_ids, vec![1]);
    assert!(project.validate_chains().is_ok());
}

#[test]
fn topology_changes_preserve_unaffected_scene_patterns() {
    let mut project = Project::demo();
    let unaffected_chain_index = 2;
    let alternate_steps = vec![vec![Step::active_with_pitch(77); 16]];
    let alternate = project
        .add_chain_pattern(
            unaffected_chain_index,
            "UNBEIRRT".to_owned(),
            alternate_steps,
        )
        .expect("add unaffected pattern");
    let scene = project.add_scene("UNBEIRRTE SZENE".to_owned());
    let alternate_id = project.patterns[alternate].id;

    project.link_lane_with_next(0).expect("link unrelated rows");
    let shifted_chain_index = project
        .chains
        .iter()
        .position(|chain| chain.lane_ids == vec![2])
        .expect("unaffected chain remains");
    let scene_pattern_index = project.scenes[scene].pattern_by_chain[shifted_chain_index];
    assert_eq!(project.patterns[scene_pattern_index].id, alternate_id);
}

#[test]
fn generation_plan_is_ready_for_all_lanes_and_future_scenes() {
    let project = Project::demo();
    assert_eq!(
        project.generation_plan.lane_configs.len(),
        project.lanes.len()
    );
    assert_eq!(
        project.generation_plan.lane_configs[6].role,
        GeneratorRole::DrumPulse
    );
    assert_eq!(
        project.generation_plan.lane_configs[7].role,
        GeneratorRole::DrumPercussion
    );
}

#[test]
fn project_round_trip_preserves_musical_state() {
    let mut project = Project::demo();
    project.performance_timeline = vec![
        PerformanceEvent {
            pulse: 0,
            action: PerformanceAction::TransportStart,
        },
        PerformanceEvent {
            pulse: 48,
            action: PerformanceAction::LiveSkip {
                lane: 2,
                step: 11,
                enabled: true,
            },
        },
        PerformanceEvent {
            pulse: 192,
            action: PerformanceAction::TransportStop,
        },
    ];
    project.performance_takes[0].loop_block = Some(PerformanceLoop {
        start_pulse: 0,
        end_pulse: 192,
        passes: 3,
    });
    project.sync_active_performance_take();
    project.add_performance_take(true);
    project.performance_takes[1].name = "NACHTFAHRT VARIANTE".to_owned();
    let json = project.to_json().expect("serialize project");
    let restored = Project::from_json(&json).expect("deserialize project");
    assert_eq!(restored, project);
}

#[test]
fn performance_takes_can_be_created_switched_and_removed() {
    let mut project = Project::demo();
    project.performance_timeline.push(PerformanceEvent {
        pulse: 24,
        action: PerformanceAction::TransportStart,
    });
    project.sync_active_performance_take();

    let second = project.add_performance_take(false);
    assert_eq!(second, 1);
    assert!(project.performance_timeline.is_empty());

    project.performance_timeline.push(PerformanceEvent {
        pulse: 96,
        action: PerformanceAction::TransportStop,
    });
    project.sync_active_performance_take();
    assert!(project.select_performance_take(0));
    assert_eq!(project.performance_timeline[0].pulse, 24);
    assert!(project.remove_active_performance_take());
    assert_eq!(project.performance_takes.len(), 1);
    assert_eq!(project.performance_timeline[0].pulse, 96);
}

#[test]
fn song_arrangement_accepts_take_and_scene_clips() {
    let mut project = Project::demo();
    let take_id = project.performance_takes[0].id;
    let scene_id = project.scenes[0].id;
    let take_clip = project.add_song_clip(SongClipSource::Take { take_id });
    let scene_clip = project.add_song_clip(SongClipSource::Scene { scene_id });

    project.song_clips[take_clip].bars = 8;
    project.song_clips[take_clip].repeats = 2;
    project.song_clips[take_clip].transpose = 7;
    project.song_clips[scene_clip].transition = ClipTransition::Restart;

    assert_eq!(project.song_clips.len(), 2);
    assert_eq!(project.song_length_pulses(), 20 * 96);
    assert_eq!(project.song_clips[take_clip].transpose, 7);
    assert_eq!(
        project.song_clips[scene_clip].transition,
        ClipTransition::Restart
    );
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
    project
        .link_lane_with_next(0)
        .expect("prepare a two-row chain");
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

#[test]
fn scenes_capture_and_restore_all_chain_patterns() {
    let mut project = Project::demo();
    let original = project.active_pattern_by_chain.clone();
    let alternate_steps = project.chains[0]
        .lane_ids
        .iter()
        .map(|lane_id| project.lanes[*lane_id].steps.clone())
        .collect();
    let alternate = project
        .add_chain_pattern(0, "ALTERNATE".to_owned(), alternate_steps)
        .expect("add pattern");
    let scene = project.add_scene("ALTERNATE SCENE".to_owned());
    assert_eq!(project.scenes[scene].pattern_by_chain[0], alternate);

    project
        .apply_chain_pattern(0, original[0])
        .expect("restore original pattern");
    project.apply_scene(scene).expect("apply scene");
    assert_eq!(project.active_pattern_by_chain[0], alternate);
    assert_eq!(project.active_scene, Some(scene));
}
