use std::fs;
use std::path::Path;

use crate::engine::probability_hits;
use crate::model::{LaneKind, PerformanceAction, Project, SongClipSource, StepDivision, StepState};

const PPQ: u16 = 480;

#[derive(Clone)]
struct Event {
    tick: u64,
    order: u8,
    data: Vec<u8>,
}

pub fn export_song(project: &Project, path: &Path) -> Result<(), String> {
    if project.song_clips.is_empty() {
        return Err("Der Song enthält noch keine Clips".to_owned());
    }
    let mut tracks = vec![tempo_track(project.bpm)];
    for chain_index in 0..project.chains.len() {
        tracks.push(render_chain(project, chain_index)?);
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"MThd");
    bytes.extend_from_slice(&6u32.to_be_bytes());
    bytes.extend_from_slice(&1u16.to_be_bytes());
    bytes.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&PPQ.to_be_bytes());
    for track in tracks {
        bytes.extend_from_slice(b"MTrk");
        bytes.extend_from_slice(&(track.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&track);
    }
    fs::write(path, bytes)
        .map_err(|error| format!("MIDI-Datei konnte nicht geschrieben werden: {error}"))
}

fn tempo_track(bpm: f32) -> Vec<u8> {
    let micros = (60_000_000.0 / bpm.clamp(20.0, 400.0)).round() as u32;
    let mut track = vec![0, 0xFF, 0x03, 5];
    track.extend_from_slice(b"KLAUS");
    track.extend_from_slice(&[0, 0xFF, 0x51, 3]);
    track.extend_from_slice(&micros.to_be_bytes()[1..]);
    track.extend_from_slice(&[0, 0xFF, 0x58, 4, 4, 2, 24, 8]);
    track.extend_from_slice(&[0, 0xFF, 0x2F, 0]);
    track
}

fn render_chain(project: &Project, chain_index: usize) -> Result<Vec<u8>, String> {
    let chain = project.chains.get(chain_index).ok_or("Unbekannte Kette")?;
    let channel = chain.route.channel.clamp(1, 16) - 1;
    let mut events = Vec::new();
    add_text(&mut events, 0, 0x03, &chain.name);
    if chain.route.sound.enabled {
        if let Some(msb) = chain.route.sound.bank_msb {
            events.push(Event {
                tick: 0,
                order: 0,
                data: vec![0xB0 | channel, 0, msb],
            });
        }
        if let Some(lsb) = chain.route.sound.bank_lsb {
            events.push(Event {
                tick: 0,
                order: 0,
                data: vec![0xB0 | channel, 32, lsb],
            });
        }
        events.push(Event {
            tick: 0,
            order: 0,
            data: vec![0xC0 | channel, chain.route.sound.program],
        });
    }
    let mut state = project.clone();
    let mut clip_start = 0u64;
    for clip in &project.song_clips {
        apply_clip_source(&mut state, clip.source);
        let clip_ticks = clip.total_pulses() * 20;
        let step_ticks = division_ticks(chain.division);
        let steps: Vec<_> = chain
            .lane_ids
            .iter()
            .filter_map(|lane| state.lanes.get(*lane))
            .flat_map(|lane| lane.steps.iter().map(move |step| (lane.kind, step)))
            .filter(|(_, step)| step.state != StepState::Skip)
            .collect();
        if steps.is_empty() {
            clip_start += clip_ticks;
            continue;
        }
        let mut tick = 0u64;
        let mut cycle = 0u64;
        while tick < clip_ticks {
            let index = ((tick / step_ticks) as usize) % steps.len();
            if index == 0 && tick > 0 {
                cycle += 1;
            }
            let (kind, step) = steps[index];
            if step.state == StepState::Active
                && probability_hits(58, chain_index, index, cycle, step.probability)
            {
                let ratchets = u64::from(step.ratchets.clamp(1, 8));
                let slice = (step_ticks / ratchets).max(1);
                for ratchet in 0..ratchets {
                    for note in &step.notes {
                        let pitch = if kind == LaneKind::Melody {
                            (i16::from(note.pitch) + i16::from(clip.transpose)).clamp(0, 127) as u8
                        } else {
                            note.pitch
                        };
                        let on = clip_start + tick + ratchet * slice;
                        let off = on
                            + ((slice as f32 * note.gate.clamp(0.05, 0.98)).round() as u64).max(1);
                        events.push(Event {
                            tick: on,
                            order: 1,
                            data: vec![0x90 | channel, pitch, note.velocity],
                        });
                        events.push(Event {
                            tick: off,
                            order: 0,
                            data: vec![0x80 | channel, pitch, 0],
                        });
                    }
                }
            }
            tick += step_ticks;
        }
        clip_start += clip_ticks;
    }
    events.sort_by_key(|event| (event.tick, event.order));
    encode_track(events, clip_start)
}

fn apply_clip_source(project: &mut Project, source: SongClipSource) {
    match source {
        SongClipSource::Scene { scene_id } => {
            if let Some(index) = project.scenes.iter().position(|scene| scene.id == scene_id) {
                let _ = project.apply_scene(index);
            }
        }
        SongClipSource::Take { take_id } => {
            let actions = project
                .performance_takes
                .iter()
                .find(|take| take.id == take_id)
                .map(|take| {
                    take.events
                        .iter()
                        .map(|event| event.action.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            for action in actions {
                match action {
                    PerformanceAction::SceneLaunch { scene_id } => {
                        if let Some(index) =
                            project.scenes.iter().position(|scene| scene.id == scene_id)
                        {
                            let _ = project.apply_scene(index);
                        }
                    }
                    PerformanceAction::PatternChange {
                        chain_id,
                        pattern_id,
                    } => {
                        let chain = project.chains.iter().position(|item| item.id == chain_id);
                        let pattern = project
                            .patterns
                            .iter()
                            .position(|item| item.id == pattern_id);
                        if let (Some(chain), Some(pattern)) = (chain, pattern) {
                            let _ = project.apply_chain_pattern(chain, pattern);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn division_ticks(value: StepDivision) -> u64 {
    match value {
        StepDivision::Quarter => 480,
        StepDivision::Eighth => 240,
        StepDivision::EighthTriplet => 160,
        StepDivision::Sixteenth => 120,
        StepDivision::SixteenthTriplet => 80,
        StepDivision::ThirtySecond => 60,
    }
}

fn add_text(events: &mut Vec<Event>, tick: u64, kind: u8, text: &str) {
    let mut data = vec![0xFF, kind];
    write_vlq(text.len() as u64, &mut data);
    data.extend_from_slice(text.as_bytes());
    events.push(Event {
        tick,
        order: 0,
        data,
    });
}
fn encode_track(events: Vec<Event>, end_tick: u64) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut last = 0;
    for event in events {
        write_vlq(event.tick.saturating_sub(last), &mut out);
        out.extend_from_slice(&event.data);
        last = event.tick;
    }
    write_vlq(end_tick.saturating_sub(last), &mut out);
    out.extend_from_slice(&[0xFF, 0x2F, 0]);
    Ok(out)
}
fn write_vlq(mut value: u64, out: &mut Vec<u8>) {
    let mut buffer = [0u8; 10];
    let mut index = 9;
    buffer[index] = (value & 0x7F) as u8;
    while {
        value >>= 7;
        value != 0
    } {
        index -= 1;
        buffer[index] = ((value & 0x7F) as u8) | 0x80;
    }
    out.extend_from_slice(&buffer[index..]);
}

#[cfg(test)]
mod tests {
    use super::export_song;
    use crate::model::{Project, SongClipSource};
    #[test]
    fn exports_type_one_song() {
        let mut project = Project::demo();
        let scene_id = project.scenes[0].id;
        project.add_song_clip(SongClipSource::Scene { scene_id });
        let path = std::env::temp_dir().join("klaus58-export-test.mid");
        export_song(&project, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"MThd");
        assert_eq!(u16::from_be_bytes([bytes[8], bytes[9]]), 1);
        let _ = std::fs::remove_file(path);
    }
}
