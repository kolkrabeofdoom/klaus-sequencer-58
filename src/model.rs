use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const LANE_COUNT: usize = 8;
pub const STEPS_PER_LANE: usize = 16;
pub const PROJECT_FORMAT_VERSION: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LaneKind {
    Melody,
    Drums,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepState {
    Active,
    Rest,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Forward,
    Reverse,
    PingPong,
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepDivision {
    Quarter,
    Eighth,
    EighthTriplet,
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SceneLaunchQuantization {
    Immediate,
    NextStep,
    NextBeat,
    #[default]
    NextBar,
}

impl SceneLaunchQuantization {
    pub fn pulses(self) -> Option<u64> {
        match self {
            Self::Immediate => None,
            Self::NextStep => Some(6),
            Self::NextBeat => Some(24),
            Self::NextBar => Some(96),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MidiClockMode {
    #[default]
    Internal,
    Master,
    Follower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MusicalScale {
    Major,
    NaturalMinor,
    HarmonicMinor,
    #[default]
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    MajorPentatonic,
    MinorPentatonic,
    WholeTone,
    Chromatic,
}

impl MusicalScale {
    pub fn intervals(self) -> &'static [u8] {
        match self {
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
            Self::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Self::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
            Self::WholeTone => &[0, 2, 4, 6, 8, 10],
            Self::Chromatic => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        }
    }

    pub fn contains(self, root: u8, pitch: u8) -> bool {
        let interval = (12 + i16::from(pitch % 12) - i16::from(root % 12)) % 12;
        self.intervals().contains(&(interval as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GeneratorRole {
    BassPulse,
    #[default]
    Ostinato,
    Arpeggio,
    ChordPulse,
    CounterMelody,
    MelodicSatellite,
    DrumPulse,
    DrumPercussion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaneGeneratorConfig {
    pub role: GeneratorRole,
    pub min_pitch: u8,
    pub max_pitch: u8,
    pub motif_length: u8,
    pub effective_length: u8,
    pub density: u8,
    pub order: u8,
    pub movement: u8,
    pub heat: u8,
    pub drift: u8,
    pub mutation: u8,
    pub lock_notes: bool,
    pub lock_rhythm: bool,
    pub lock_expression: bool,
    pub seed_offset: u64,
}

impl LaneGeneratorConfig {
    pub fn for_lane(index: usize, kind: LaneKind) -> Self {
        if kind == LaneKind::Drums {
            return Self {
                role: if index == 6 {
                    GeneratorRole::DrumPulse
                } else {
                    GeneratorRole::DrumPercussion
                },
                min_pitch: 36,
                max_pitch: 43,
                motif_length: if index == 6 { 4 } else { 7 },
                effective_length: 16,
                density: if index == 6 { 48 } else { 38 },
                order: 76,
                movement: 28,
                heat: if index == 6 { 24 } else { 46 },
                drift: 18,
                mutation: 20,
                lock_notes: false,
                lock_rhythm: false,
                lock_expression: false,
                seed_offset: index as u64 * 5_858,
            };
        }
        let (role, min_pitch, max_pitch, motif_length, density) = match index {
            0 => (GeneratorRole::BassPulse, 36, 55, 5, 78),
            1 => (GeneratorRole::Ostinato, 40, 64, 7, 72),
            2 => (GeneratorRole::ChordPulse, 48, 76, 4, 42),
            3 => (GeneratorRole::CounterMelody, 55, 84, 7, 46),
            4 => (GeneratorRole::Arpeggio, 48, 84, 7, 76),
            _ => (GeneratorRole::MelodicSatellite, 60, 91, 5, 32),
        };
        Self {
            role,
            min_pitch,
            max_pitch,
            motif_length,
            effective_length: 16,
            density,
            order: 74,
            movement: 42,
            heat: 28,
            drift: 18,
            mutation: 20,
            lock_notes: false,
            lock_rhythm: false,
            lock_expression: false,
            seed_offset: index as u64 * 5_858,
        }
    }
}

impl Default for LaneGeneratorConfig {
    fn default() -> Self {
        Self::for_lane(0, LaneKind::Melody)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationPlan {
    pub root: u8,
    pub scale: MusicalScale,
    pub seed: u64,
    pub lane_configs: Vec<LaneGeneratorConfig>,
}

impl Default for GenerationPlan {
    fn default() -> Self {
        Self {
            root: 2,
            scale: MusicalScale::Dorian,
            seed: 58,
            lane_configs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArpMode {
    #[default]
    Up,
    Down,
    UpDown,
    Random,
    AsPlayed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArpeggiatorSettings {
    pub enabled: bool,
    pub mode: ArpMode,
    pub latch: bool,
    pub octaves: u8,
    pub division: StepDivision,
    pub gate: f32,
    pub ratchets: u8,
}

impl Default for ArpeggiatorSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: ArpMode::Up,
            latch: false,
            octaves: 1,
            division: StepDivision::Sixteenth,
            gate: 0.72,
            ratchets: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteEvent {
    pub pitch: u8,
    pub velocity: u8,
    pub gate: f32,
}

impl NoteEvent {
    pub fn new(pitch: u8) -> Self {
        Self {
            pitch,
            velocity: 100,
            gate: 0.72,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub state: StepState,
    pub notes: Vec<NoteEvent>,
    pub probability: u8,
    pub ratchets: u8,
}

impl Step {
    pub fn rest() -> Self {
        Self {
            state: StepState::Rest,
            notes: Vec::new(),
            probability: 100,
            ratchets: 1,
        }
    }

    pub fn active_with_pitch(pitch: u8) -> Self {
        Self {
            state: StepState::Active,
            notes: vec![NoteEvent::new(pitch)],
            probability: 100,
            ratchets: 1,
        }
    }

    pub fn toggle_active(&mut self, default_pitch: u8) {
        match self.state {
            StepState::Active => self.state = StepState::Rest,
            StepState::Rest | StepState::Skip => {
                self.state = StepState::Active;
                if self.notes.is_empty() {
                    self.notes.push(NoteEvent::new(default_pitch));
                }
            }
        }
    }

    pub fn toggle_note(&mut self, pitch: u8) {
        if let Some(index) = self.notes.iter().position(|note| note.pitch == pitch) {
            self.notes.remove(index);
        } else {
            self.notes.push(NoteEvent::new(pitch));
        }
        self.state = if self.notes.is_empty() {
            StepState::Rest
        } else {
            StepState::Active
        };
    }
}

impl Default for Step {
    fn default() -> Self {
        Self::rest()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub id: usize,
    pub name: String,
    pub kind: LaneKind,
    pub steps: Vec<Step>,
    pub muted: bool,
    pub solo: bool,
}

impl Lane {
    pub fn default_pitch(&self) -> u8 {
        match self.kind {
            LaneKind::Melody => 48 + self.id as u8,
            LaneKind::Drums => 36 + (self.id.saturating_sub(6) as u8) * 2,
        }
    }

    pub fn effective_length(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.state != StepState::Skip)
            .count()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiSoundTarget {
    pub enabled: bool,
    pub bank_msb: Option<u8>,
    pub bank_lsb: Option<u8>,
    pub program: u8,
    pub send_on_connect: bool,
}

impl Default for MidiSoundTarget {
    fn default() -> Self {
        Self {
            enabled: false,
            bank_msb: Some(0),
            bank_lsb: Some(0),
            program: 0,
            send_on_connect: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MidiRoute {
    pub output_name: Option<String>,
    pub channel: u8,
    #[serde(default)]
    pub sound: MidiSoundTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub id: usize,
    pub name: String,
    pub lane_ids: Vec<usize>,
    pub route: MidiRoute,
    pub division: StepDivision,
    pub swing: f32,
    pub direction: Direction,
    #[serde(default)]
    pub arpeggiator: ArpeggiatorSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainPattern {
    pub id: usize,
    pub chain_id: usize,
    pub name: String,
    pub lane_steps: Vec<Vec<Step>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: usize,
    pub name: String,
    pub pattern_by_chain: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerformanceAction {
    SceneLaunch {
        scene_id: usize,
    },
    PatternChange {
        chain_id: usize,
        pattern_id: usize,
    },
    LiveSkip {
        lane: usize,
        step: usize,
        enabled: bool,
    },
    TransportStart,
    TransportStop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceEvent {
    pub pulse: u64,
    pub action: PerformanceAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceLoop {
    pub start_pulse: u64,
    pub end_pulse: u64,
    pub passes: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceTake {
    pub id: usize,
    pub name: String,
    pub events: Vec<PerformanceEvent>,
    #[serde(default)]
    pub loop_block: Option<PerformanceLoop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SongClipSource {
    Take { take_id: usize },
    Scene { scene_id: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ClipTransition {
    #[default]
    Legato,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongClip {
    pub id: usize,
    pub name: String,
    pub source: SongClipSource,
    pub bars: u16,
    pub repeats: u8,
    pub transpose: i8,
    pub color_index: u8,
    #[serde(default)]
    pub transition: ClipTransition,
}

impl SongClip {
    pub fn total_pulses(&self) -> u64 {
        u64::from(self.bars.max(1)) * 96 * u64::from(self.repeats.max(1))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub title: String,
    pub bpm: f32,
    pub global_swing: f32,
    pub lanes: Vec<Lane>,
    pub chains: Vec<Chain>,
    #[serde(default)]
    pub patterns: Vec<ChainPattern>,
    #[serde(default)]
    pub active_pattern_by_chain: Vec<usize>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub active_scene: Option<usize>,
    #[serde(default)]
    pub scene_launch_quantization: SceneLaunchQuantization,
    #[serde(default)]
    pub midi_clock_mode: MidiClockMode,
    #[serde(default)]
    pub performance_timeline: Vec<PerformanceEvent>,
    #[serde(default)]
    pub performance_takes: Vec<PerformanceTake>,
    #[serde(default)]
    pub active_performance_take: usize,
    #[serde(default)]
    pub song_clips: Vec<SongClip>,
    #[serde(default)]
    pub generation_plan: GenerationPlan,
}

struct ChainTopologySnapshot {
    chain_ids: Vec<usize>,
    active_pattern_ids: Vec<Option<usize>>,
    scene_pattern_ids: Vec<Vec<Option<usize>>>,
}

impl Project {
    pub fn demo() -> Self {
        let names = [
            "BASS A", "BASS B", "CHORDS", "LEAD", "ARP", "AUX", "DRUM A", "DRUM B",
        ];
        let patterns: [&[usize]; LANE_COUNT] = [
            &[0, 2, 5, 7, 8, 11, 14],
            &[0, 3, 5, 9, 12, 15],
            &[1, 4, 6, 10, 13],
            &[0, 2, 4, 7, 9, 12, 14],
            &[0, 3, 6, 8, 10, 13, 15],
            &[2, 5, 8, 11, 14],
            &[0, 4, 7, 10, 13],
            &[2, 6, 8, 12, 15],
        ];

        let lanes: Vec<Lane> = names
            .iter()
            .enumerate()
            .map(|(id, name)| {
                let kind = if id < 6 {
                    LaneKind::Melody
                } else {
                    LaneKind::Drums
                };
                let default_pitch = if kind == LaneKind::Melody {
                    48 + id as u8
                } else {
                    36 + (id - 6) as u8 * 2
                };
                let mut steps = vec![Step::rest(); STEPS_PER_LANE];
                for step in patterns[id] {
                    steps[*step] = Step::active_with_pitch(default_pitch);
                }
                if id == 3 {
                    steps[11].state = StepState::Skip;
                }
                if id == 6 {
                    steps[4].ratchets = 4;
                    steps[10].probability = 72;
                }
                Lane {
                    id,
                    name: (*name).to_owned(),
                    kind,
                    steps,
                    muted: false,
                    solo: false,
                }
            })
            .collect();

        let chains = lanes
            .iter()
            .map(|lane| Chain {
                id: lane.id,
                name: lane.name.clone(),
                lane_ids: vec![lane.id],
                route: MidiRoute {
                    output_name: None,
                    channel: if lane.kind == LaneKind::Drums {
                        10
                    } else {
                        (lane.id as u8 + 1).clamp(1, 16)
                    },
                    sound: MidiSoundTarget::default(),
                },
                division: StepDivision::Sixteenth,
                swing: if matches!(lane.id, 0 | 6) { 0.54 } else { 0.5 },
                direction: Direction::Forward,
                arpeggiator: if lane.id == 4 {
                    ArpeggiatorSettings {
                        enabled: true,
                        ..ArpeggiatorSettings::default()
                    }
                } else {
                    ArpeggiatorSettings::default()
                },
            })
            .collect();

        let mut project = Self {
            format_version: PROJECT_FORMAT_VERSION,
            title: "Night Drive".to_owned(),
            bpm: 124.0,
            global_swing: 0.54,
            lanes,
            chains,
            patterns: Vec::new(),
            active_pattern_by_chain: Vec::new(),
            scenes: Vec::new(),
            active_scene: None,
            scene_launch_quantization: SceneLaunchQuantization::default(),
            midi_clock_mode: MidiClockMode::default(),
            performance_timeline: Vec::new(),
            performance_takes: Vec::new(),
            active_performance_take: 0,
            song_clips: Vec::new(),
            generation_plan: GenerationPlan::default(),
        };
        project.ensure_patterns();
        project.ensure_scenes();
        project.ensure_generation_plan();
        project.ensure_performance_takes();
        project
    }

    pub fn ensure_performance_takes(&mut self) {
        if self.performance_takes.is_empty() {
            self.performance_takes.push(PerformanceTake {
                id: 0,
                name: "TAKE 01".to_owned(),
                events: self.performance_timeline.clone(),
                loop_block: None,
            });
        }
        self.active_performance_take = self
            .active_performance_take
            .min(self.performance_takes.len().saturating_sub(1));
        self.performance_timeline = self.performance_takes[self.active_performance_take]
            .events
            .clone();
    }

    pub fn sync_active_performance_take(&mut self) {
        if let Some(take) = self.performance_takes.get_mut(self.active_performance_take) {
            take.events = self.performance_timeline.clone();
        }
    }

    pub fn select_performance_take(&mut self, index: usize) -> bool {
        if index >= self.performance_takes.len() {
            return false;
        }
        self.sync_active_performance_take();
        self.active_performance_take = index;
        self.performance_timeline = self.performance_takes[index].events.clone();
        true
    }

    pub fn add_performance_take(&mut self, duplicate_current: bool) -> usize {
        self.sync_active_performance_take();
        let id = self
            .performance_takes
            .iter()
            .map(|take| take.id)
            .max()
            .map_or(0, |id| id + 1);
        let (events, loop_block) = if duplicate_current {
            let current = &self.performance_takes[self.active_performance_take];
            (current.events.clone(), current.loop_block.clone())
        } else {
            (Vec::new(), None)
        };
        self.performance_takes.push(PerformanceTake {
            id,
            name: format!("TAKE {:02}", self.performance_takes.len() + 1),
            events,
            loop_block,
        });
        self.active_performance_take = self.performance_takes.len() - 1;
        self.performance_timeline = self.performance_takes[self.active_performance_take]
            .events
            .clone();
        self.active_performance_take
    }

    pub fn remove_active_performance_take(&mut self) -> bool {
        if self.performance_takes.len() <= 1 {
            return false;
        }
        self.performance_takes.remove(self.active_performance_take);
        self.active_performance_take = self
            .active_performance_take
            .min(self.performance_takes.len() - 1);
        self.performance_timeline = self.performance_takes[self.active_performance_take]
            .events
            .clone();
        true
    }

    pub fn active_performance_loop(&self) -> Option<&PerformanceLoop> {
        self.performance_takes
            .get(self.active_performance_take)
            .and_then(|take| take.loop_block.as_ref())
    }

    pub fn add_song_clip(&mut self, source: SongClipSource) -> usize {
        let id = self
            .song_clips
            .iter()
            .map(|clip| clip.id)
            .max()
            .map_or(0, |id| id + 1);
        let (name, bars) = match source {
            SongClipSource::Take { take_id } => {
                let take = self
                    .performance_takes
                    .iter()
                    .find(|take| take.id == take_id);
                let name = take.map_or_else(|| "TAKE-CLIP".to_owned(), |take| take.name.clone());
                let pulses = take
                    .and_then(|take| take.events.last())
                    .map_or(96, |event| event.pulse.max(1));
                (name, pulses.div_ceil(96).clamp(1, 256) as u16)
            }
            SongClipSource::Scene { scene_id } => {
                let name = self
                    .scenes
                    .iter()
                    .find(|scene| scene.id == scene_id)
                    .map_or_else(|| "SZENEN-CLIP".to_owned(), |scene| scene.name.clone());
                (name, 4)
            }
        };
        self.song_clips.push(SongClip {
            id,
            name,
            source,
            bars,
            repeats: 1,
            transpose: 0,
            color_index: (id % 4) as u8,
            transition: ClipTransition::Legato,
        });
        self.song_clips.len() - 1
    }

    pub fn song_length_pulses(&self) -> u64 {
        self.song_clips.iter().map(SongClip::total_pulses).sum()
    }

    pub fn validate_chains(&self) -> Result<(), String> {
        for chain in &self.chains {
            if chain.lane_ids.is_empty() {
                return Err(format!("Kette '{}' enthält keine Reihen", chain.name));
            }
            let first = chain.lane_ids[0];
            let Some(first_lane) = self.lanes.get(first) else {
                return Err(format!(
                    "Kette '{}' verweist auf eine unbekannte Reihe",
                    chain.name
                ));
            };
            for pair in chain.lane_ids.windows(2) {
                if pair[1] != pair[0] + 1 {
                    return Err(format!(
                        "Kette '{}' enthält nicht benachbarte Reihen",
                        chain.name
                    ));
                }
            }
            if chain
                .lane_ids
                .iter()
                .filter_map(|id| self.lanes.get(*id))
                .any(|lane| lane.kind != first_lane.kind)
            {
                return Err(format!(
                    "Kette '{}' mischt unterschiedliche Reihentypen",
                    chain.name
                ));
            }
        }
        Ok(())
    }

    pub fn can_link_lane_with_next(&self, lane_id: usize) -> bool {
        let Some(next_lane_id) = lane_id.checked_add(1) else {
            return false;
        };
        let Some(lane) = self.lanes.get(lane_id) else {
            return false;
        };
        let Some(next_lane) = self.lanes.get(next_lane_id) else {
            return false;
        };
        if lane.kind != next_lane.kind {
            return false;
        }
        let Some(chain) = self
            .chains
            .iter()
            .find(|chain| chain.lane_ids.contains(&lane_id))
        else {
            return false;
        };
        let Some(next_chain) = self
            .chains
            .iter()
            .find(|chain| chain.lane_ids.contains(&next_lane_id))
        else {
            return false;
        };
        chain.id != next_chain.id
            && chain.lane_ids.last() == Some(&lane_id)
            && next_chain.lane_ids.first() == Some(&next_lane_id)
    }

    pub fn link_lane_with_next(&mut self, lane_id: usize) -> Result<(), String> {
        if !self.can_link_lane_with_next(lane_id) {
            return Err(
                "Nur benachbarte, getrennte Reihen desselben Typs lassen sich verketten".to_owned(),
            );
        }
        let snapshot = self.chain_topology_snapshot();
        let left_index = self
            .chains
            .iter()
            .position(|chain| chain.lane_ids.contains(&lane_id))
            .ok_or_else(|| "Reihe hat keine Kette".to_owned())?;
        let right_index = self
            .chains
            .iter()
            .position(|chain| chain.lane_ids.contains(&(lane_id + 1)))
            .ok_or_else(|| "Folgereihe hat keine Kette".to_owned())?;
        let left_id = self.chains[left_index].id;
        let right_id = self.chains[right_index].id;
        let right_lanes = self.chains[right_index].lane_ids.clone();
        self.chains[left_index].lane_ids.extend(right_lanes);
        let first_lane = self.chains[left_index].lane_ids[0];
        let last_lane = *self.chains[left_index]
            .lane_ids
            .last()
            .unwrap_or(&first_lane);
        self.chains[left_index].name = format!(
            "{} · {}",
            self.lanes[first_lane].name, self.lanes[last_lane].name
        );
        self.chains.remove(right_index);
        self.finish_chain_topology_change(snapshot, &[left_id, right_id], &[left_id]);
        self.validate_chains()
    }

    pub fn can_split_chain_for_lane(&self, lane_id: usize) -> bool {
        self.chains
            .iter()
            .find(|chain| chain.lane_ids.contains(&lane_id))
            .is_some_and(|chain| chain.lane_ids.len() > 1)
    }

    pub fn split_chain_for_lane(&mut self, lane_id: usize) -> Result<(), String> {
        let chain_index = self
            .chains
            .iter()
            .position(|chain| chain.lane_ids.contains(&lane_id))
            .ok_or_else(|| "Reihe hat keine Kette".to_owned())?;
        if self.chains[chain_index].lane_ids.len() <= 1 {
            return Err("Diese Reihe ist bereits unabhängig".to_owned());
        }
        let snapshot = self.chain_topology_snapshot();
        let original = self.chains.remove(chain_index);
        let old_id = original.id;
        let mut next_id = self
            .chains
            .iter()
            .map(|chain| chain.id)
            .max()
            .map_or(old_id + 1, |id| id.max(old_id) + 1);
        let mut new_ids = Vec::with_capacity(original.lane_ids.len());
        for (offset, lane_id) in original.lane_ids.iter().copied().enumerate() {
            let mut chain = original.clone();
            chain.id = if offset == 0 {
                old_id
            } else {
                let id = next_id;
                next_id += 1;
                id
            };
            chain.name = self.lanes[lane_id].name.clone();
            chain.lane_ids = vec![lane_id];
            new_ids.push(chain.id);
            self.chains.insert(chain_index + offset, chain);
        }
        self.finish_chain_topology_change(snapshot, &[old_id], &new_ids);
        self.validate_chains()
    }

    fn chain_topology_snapshot(&mut self) -> ChainTopologySnapshot {
        self.sync_all_active_patterns();
        let chain_ids = self.chains.iter().map(|chain| chain.id).collect();
        let active_pattern_ids = self
            .active_pattern_by_chain
            .iter()
            .map(|index| self.patterns.get(*index).map(|pattern| pattern.id))
            .collect();
        let scene_pattern_ids = self
            .scenes
            .iter()
            .map(|scene| {
                scene
                    .pattern_by_chain
                    .iter()
                    .map(|index| self.patterns.get(*index).map(|pattern| pattern.id))
                    .collect()
            })
            .collect();
        ChainTopologySnapshot {
            chain_ids,
            active_pattern_ids,
            scene_pattern_ids,
        }
    }

    fn finish_chain_topology_change(
        &mut self,
        snapshot: ChainTopologySnapshot,
        affected_old_ids: &[usize],
        affected_new_ids: &[usize],
    ) {
        let affected_old: BTreeSet<usize> = affected_old_ids.iter().copied().collect();
        let affected_new: BTreeSet<usize> = affected_new_ids.iter().copied().collect();
        self.performance_timeline.retain(|event| {
            !matches!(
                &event.action,
                PerformanceAction::PatternChange { chain_id, .. }
                    if affected_old.contains(chain_id)
            )
        });
        for take in &mut self.performance_takes {
            take.events.retain(|event| {
                !matches!(
                    &event.action,
                    PerformanceAction::PatternChange { chain_id, .. }
                        if affected_old.contains(chain_id)
                )
            });
        }
        self.patterns
            .retain(|pattern| !affected_old.contains(&pattern.chain_id));
        let next_pattern_id = self
            .patterns
            .iter()
            .map(|pattern| pattern.id)
            .max()
            .map_or(0, |id| id + 1);
        for (pattern_id, chain) in (next_pattern_id..).zip(
            self.chains
                .iter()
                .filter(|chain| affected_new.contains(&chain.id)),
        ) {
            let lane_steps = chain
                .lane_ids
                .iter()
                .map(|lane_id| self.lanes[*lane_id].steps.clone())
                .collect();
            self.patterns.push(ChainPattern {
                id: pattern_id,
                chain_id: chain.id,
                name: "PATTERN 01".to_owned(),
                lane_steps,
            });
        }

        self.active_pattern_by_chain = self
            .chains
            .iter()
            .map(|chain| {
                if affected_new.contains(&chain.id) {
                    return self
                        .patterns
                        .iter()
                        .position(|pattern| pattern.chain_id == chain.id)
                        .unwrap_or(0);
                }
                let stable_pattern_id = snapshot
                    .chain_ids
                    .iter()
                    .position(|id| *id == chain.id)
                    .and_then(|index| snapshot.active_pattern_ids.get(index).copied().flatten());
                stable_pattern_id
                    .and_then(|id| self.patterns.iter().position(|pattern| pattern.id == id))
                    .or_else(|| {
                        self.patterns
                            .iter()
                            .position(|pattern| pattern.chain_id == chain.id)
                    })
                    .unwrap_or(0)
            })
            .collect();

        for (scene_index, scene) in self.scenes.iter_mut().enumerate() {
            scene.pattern_by_chain = self
                .chains
                .iter()
                .enumerate()
                .map(|(chain_index, chain)| {
                    if affected_new.contains(&chain.id) {
                        return self.active_pattern_by_chain[chain_index];
                    }
                    let stable_pattern_id = snapshot
                        .chain_ids
                        .iter()
                        .position(|id| *id == chain.id)
                        .and_then(|old_chain_index| {
                            snapshot
                                .scene_pattern_ids
                                .get(scene_index)
                                .and_then(|patterns| patterns.get(old_chain_index))
                                .copied()
                                .flatten()
                        });
                    stable_pattern_id
                        .and_then(|id| self.patterns.iter().position(|pattern| pattern.id == id))
                        .unwrap_or(self.active_pattern_by_chain[chain_index])
                })
                .collect();
        }
    }

    pub fn ensure_lane_coverage(&mut self) {
        let mut next_id = self
            .chains
            .iter()
            .map(|chain| chain.id)
            .max()
            .map_or(0, |id| id + 1);
        for lane in &self.lanes {
            if self
                .chains
                .iter()
                .any(|chain| chain.lane_ids.contains(&lane.id))
            {
                continue;
            }
            self.chains.push(Chain {
                id: next_id,
                name: lane.name.clone(),
                lane_ids: vec![lane.id],
                route: MidiRoute {
                    output_name: None,
                    channel: if lane.kind == LaneKind::Drums {
                        10
                    } else {
                        (lane.id as u8 + 1).clamp(1, 16)
                    },
                    sound: MidiSoundTarget::default(),
                },
                division: StepDivision::Sixteenth,
                swing: self.global_swing.clamp(0.5, 0.75),
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            });
            next_id += 1;
        }
    }

    pub fn ensure_patterns(&mut self) {
        for (chain_index, chain) in self.chains.iter().enumerate() {
            let pattern_index = self
                .patterns
                .iter()
                .position(|pattern| pattern.chain_id == chain.id)
                .unwrap_or_else(|| {
                    let id = self
                        .patterns
                        .iter()
                        .map(|pattern| pattern.id)
                        .max()
                        .map_or(0, |id| id + 1);
                    let lane_steps = chain
                        .lane_ids
                        .iter()
                        .filter_map(|lane_id| self.lanes.get(*lane_id))
                        .map(|lane| lane.steps.clone())
                        .collect();
                    self.patterns.push(ChainPattern {
                        id,
                        chain_id: chain.id,
                        name: "PATTERN 01".to_owned(),
                        lane_steps,
                    });
                    self.patterns.len() - 1
                });
            if self.active_pattern_by_chain.len() <= chain_index {
                self.active_pattern_by_chain.push(pattern_index);
            } else if self
                .patterns
                .get(self.active_pattern_by_chain[chain_index])
                .is_none_or(|pattern| pattern.chain_id != chain.id)
            {
                self.active_pattern_by_chain[chain_index] = pattern_index;
            }
        }
        self.active_pattern_by_chain.truncate(self.chains.len());
    }

    pub fn ensure_generation_plan(&mut self) {
        while self.generation_plan.lane_configs.len() < self.lanes.len() {
            let index = self.generation_plan.lane_configs.len();
            let kind = self
                .lanes
                .get(index)
                .map_or(LaneKind::Melody, |lane| lane.kind);
            self.generation_plan
                .lane_configs
                .push(LaneGeneratorConfig::for_lane(index, kind));
        }
        self.generation_plan.lane_configs.truncate(self.lanes.len());
        self.generation_plan.root %= 12;
    }

    pub fn ensure_scenes(&mut self) {
        self.ensure_patterns();
        if self.scenes.is_empty() {
            self.scenes.push(Scene {
                id: 0,
                name: "SCENE 01".to_owned(),
                pattern_by_chain: self.active_pattern_by_chain.clone(),
            });
            self.active_scene = Some(0);
            return;
        }

        for scene in &mut self.scenes {
            scene.pattern_by_chain.resize(self.chains.len(), 0);
            for (chain_index, pattern_index) in scene.pattern_by_chain.iter_mut().enumerate() {
                let valid = self
                    .patterns
                    .get(*pattern_index)
                    .zip(self.chains.get(chain_index))
                    .is_some_and(|(pattern, chain)| pattern.chain_id == chain.id);
                if !valid {
                    *pattern_index = self.active_pattern_by_chain[chain_index];
                }
            }
        }
        if self
            .active_scene
            .is_some_and(|index| index >= self.scenes.len())
        {
            self.active_scene = None;
        }
    }

    pub fn add_scene(&mut self, name: String) -> usize {
        self.sync_all_active_patterns();
        let id = self
            .scenes
            .iter()
            .map(|scene| scene.id)
            .max()
            .map_or(0, |id| id + 1);
        self.scenes.push(Scene {
            id,
            name,
            pattern_by_chain: self.active_pattern_by_chain.clone(),
        });
        let index = self.scenes.len() - 1;
        self.active_scene = Some(index);
        index
    }

    pub fn capture_scene(&mut self, scene_index: usize) -> Result<(), String> {
        self.sync_all_active_patterns();
        let scene = self
            .scenes
            .get_mut(scene_index)
            .ok_or_else(|| "Unbekannte Szene".to_owned())?;
        scene.pattern_by_chain = self.active_pattern_by_chain.clone();
        self.active_scene = Some(scene_index);
        Ok(())
    }

    pub fn apply_scene(&mut self, scene_index: usize) -> Result<(), String> {
        self.sync_all_active_patterns();
        let scene = self
            .scenes
            .get(scene_index)
            .ok_or_else(|| "Unbekannte Szene".to_owned())?
            .clone();
        if scene.pattern_by_chain.len() != self.chains.len() {
            return Err("Szene hat nicht die erwartete Kettenanzahl".to_owned());
        }
        for (chain_index, pattern_index) in scene.pattern_by_chain.into_iter().enumerate() {
            self.apply_chain_pattern(chain_index, pattern_index)?;
        }
        self.active_scene = Some(scene_index);
        Ok(())
    }

    pub fn add_chain_pattern(
        &mut self,
        chain_index: usize,
        name: String,
        lane_steps: Vec<Vec<Step>>,
    ) -> Result<usize, String> {
        let chain = self
            .chains
            .get(chain_index)
            .ok_or_else(|| "Unbekannte Kette".to_owned())?;
        if lane_steps.len() != chain.lane_ids.len()
            || lane_steps.iter().any(|steps| steps.len() != STEPS_PER_LANE)
        {
            return Err("Pattern hat nicht die erwartete Kettenlänge".to_owned());
        }
        let id = self
            .patterns
            .iter()
            .map(|pattern| pattern.id)
            .max()
            .map_or(0, |id| id + 1);
        self.patterns.push(ChainPattern {
            id,
            chain_id: chain.id,
            name,
            lane_steps,
        });
        let pattern_index = self.patterns.len() - 1;
        self.ensure_patterns();
        self.active_pattern_by_chain[chain_index] = pattern_index;
        self.apply_chain_pattern(chain_index, pattern_index)?;
        Ok(pattern_index)
    }

    pub fn apply_chain_pattern(
        &mut self,
        chain_index: usize,
        pattern_index: usize,
    ) -> Result<(), String> {
        let chain = self
            .chains
            .get(chain_index)
            .ok_or_else(|| "Unbekannte Kette".to_owned())?
            .clone();
        let pattern = self
            .patterns
            .get(pattern_index)
            .ok_or_else(|| "Unbekanntes Pattern".to_owned())?
            .clone();
        if pattern.chain_id != chain.id || pattern.lane_steps.len() != chain.lane_ids.len() {
            return Err("Pattern gehört nicht zu dieser Kette".to_owned());
        }
        for (lane_id, steps) in chain.lane_ids.iter().zip(pattern.lane_steps) {
            if steps.len() != STEPS_PER_LANE {
                return Err("Pattern enthält eine ungültige Step-Anzahl".to_owned());
            }
            let lane = self
                .lanes
                .get_mut(*lane_id)
                .ok_or_else(|| "Pattern verweist auf eine unbekannte Reihe".to_owned())?;
            lane.steps = steps;
        }
        self.active_pattern_by_chain[chain_index] = pattern_index;
        Ok(())
    }

    pub fn sync_active_pattern_from_lanes(&mut self, chain_index: usize) -> Result<(), String> {
        let chain = self
            .chains
            .get(chain_index)
            .ok_or_else(|| "Unbekannte Kette".to_owned())?;
        let pattern_index = *self
            .active_pattern_by_chain
            .get(chain_index)
            .ok_or_else(|| "Kette hat kein aktives Pattern".to_owned())?;
        let lane_steps: Vec<Vec<Step>> = chain
            .lane_ids
            .iter()
            .map(|lane_id| {
                self.lanes
                    .get(*lane_id)
                    .map(|lane| lane.steps.clone())
                    .ok_or_else(|| "Kette verweist auf eine unbekannte Reihe".to_owned())
            })
            .collect::<Result<_, _>>()?;
        let pattern = self
            .patterns
            .get_mut(pattern_index)
            .ok_or_else(|| "Aktives Pattern wurde nicht gefunden".to_owned())?;
        if pattern.chain_id != chain.id {
            return Err("Aktives Pattern gehört nicht zu dieser Kette".to_owned());
        }
        pattern.lane_steps = lane_steps;
        Ok(())
    }

    pub fn sync_all_active_patterns(&mut self) {
        for chain_index in 0..self.chains.len() {
            let _ = self.sync_active_pattern_from_lanes(chain_index);
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(value: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(value)
    }
}

impl Default for Project {
    fn default() -> Self {
        Self::demo()
    }
}
