use serde::{Deserialize, Serialize};

pub const LANE_COUNT: usize = 8;
pub const STEPS_PER_LANE: usize = 16;
pub const PROJECT_FORMAT_VERSION: u32 = 3;

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
pub struct MidiRoute {
    pub output_name: Option<String>,
    pub channel: u8,
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
}

impl Project {
    pub fn demo() -> Self {
        let names = [
            "BASS A",
            "BASS A · 17–32",
            "CHORDS",
            "LEAD",
            "ARP",
            "AUX",
            "DRUM A",
            "DRUM B",
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

        let lanes = names
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

        let chains = vec![
            Chain {
                id: 0,
                name: "BASS A".to_owned(),
                lane_ids: vec![0, 1],
                route: MidiRoute {
                    output_name: None,
                    channel: 3,
                },
                division: StepDivision::Sixteenth,
                swing: 0.54,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            },
            Chain {
                id: 1,
                name: "CHORDS".to_owned(),
                lane_ids: vec![2],
                route: MidiRoute {
                    output_name: None,
                    channel: 6,
                },
                division: StepDivision::Sixteenth,
                swing: 0.5,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            },
            Chain {
                id: 2,
                name: "LEAD".to_owned(),
                lane_ids: vec![3],
                route: MidiRoute {
                    output_name: None,
                    channel: 4,
                },
                division: StepDivision::Sixteenth,
                swing: 0.5,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            },
            Chain {
                id: 3,
                name: "ARP".to_owned(),
                lane_ids: vec![4],
                route: MidiRoute {
                    output_name: None,
                    channel: 5,
                },
                division: StepDivision::Sixteenth,
                swing: 0.5,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings {
                    enabled: true,
                    ..ArpeggiatorSettings::default()
                },
            },
            Chain {
                id: 4,
                name: "AUX".to_owned(),
                lane_ids: vec![5],
                route: MidiRoute {
                    output_name: None,
                    channel: 7,
                },
                division: StepDivision::Sixteenth,
                swing: 0.5,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            },
            Chain {
                id: 5,
                name: "DRUMS".to_owned(),
                lane_ids: vec![6, 7],
                route: MidiRoute {
                    output_name: None,
                    channel: 10,
                },
                division: StepDivision::Sixteenth,
                swing: 0.54,
                direction: Direction::Forward,
                arpeggiator: ArpeggiatorSettings::default(),
            },
        ];

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
        };
        project.ensure_patterns();
        project.ensure_scenes();
        project
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
