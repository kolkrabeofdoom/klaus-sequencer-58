use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, TextureHandle};

use crate::engine::{
    PreviewEngine, StepTrigger, division_seconds, next_timed_index, probability_hits,
};
use crate::midi::{
    MidiEvent, MidiInputEvent, MidiInputMessage, MidiInputService, MidiService, MidiStatus,
    NoteInput, decode_note, note_off, note_on,
};
use crate::model::{
    ArpMode, Direction, LaneKind, NoteEvent, Project, STEPS_PER_LANE, Step, StepDivision, StepState,
};
use crate::persistence::{default_autosave_path, load_project, save_project_atomic};
use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkMode {
    Edit,
    Perform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Live,
    ChordCapture,
    StepRecord,
    QuantizedLive,
}

pub struct KlausApp {
    pub project: Project,
    engine: PreviewEngine,
    mode: WorkMode,
    selected_lane: usize,
    selected_step: usize,
    live_skips: BTreeSet<(usize, usize)>,
    global_live_skip: bool,
    about_open: bool,
    last_frame: Instant,
    splash_texture: TextureHandle,
    midi: MidiService,
    midi_ports: Vec<String>,
    selected_output: Option<String>,
    midi_status: MidiStatus,
    autosave_path: PathBuf,
    last_autosave: Instant,
    last_saved_json: String,
    save_status: String,
    midi_input: MidiInputService,
    midi_input_ports: Vec<String>,
    selected_input: Option<String>,
    midi_input_status: MidiStatus,
    input_mode: InputMode,
    midi_thru: bool,
    held_notes: BTreeSet<u8>,
    capture_notes: BTreeMap<u8, u8>,
    arp_latched_notes: BTreeSet<u8>,
    arp_note_order: Vec<u8>,
    arp_velocities: BTreeMap<u8, u8>,
    arp_elapsed: f64,
    arp_cursor: usize,
    arp_random_state: u64,
    arp_running: bool,
    arp_status: String,
    promo_capture: bool,
}

impl KlausApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let image =
            image::load_from_memory(include_bytes!("../assets/klaussequencer58-info-splash.png"))
                .expect("kanonische Splashgrafik muss lesbar sein")
                .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let splash_texture = cc.egui_ctx.load_texture(
            "klaussequencer58-info-splash",
            color_image,
            egui::TextureOptions::LINEAR,
        );

        let autosave_path = default_autosave_path();
        let (mut project, save_status) = if autosave_path.exists() {
            match load_project(&autosave_path) {
                Ok(project) => (project, "AUTOSAVE WIEDERHERGESTELLT".to_owned()),
                Err(error) => (Project::demo(), format!("AUTOSAVE-FEHLER · {error}")),
            }
        } else {
            (Project::demo(), "AUTOSAVE BEREIT".to_owned())
        };
        let promo_capture = std::env::var_os("KLAUS_PROMO_CAPTURE").is_some();
        if promo_capture {
            let promo_notes: &[(usize, &[u8])] = &[
                (0, &[48, 52, 55]),
                (1, &[50]),
                (2, &[53, 57]),
                (3, &[51, 55, 58]),
                (5, &[52, 55]),
                (6, &[54]),
                (8, &[48, 55]),
                (10, &[50, 53, 57]),
                (13, &[52, 59]),
                (15, &[55]),
            ];
            if let Some(lane) = project.lanes.get_mut(4) {
                for step in &mut lane.steps {
                    *step = Step::rest();
                }
                for (step_index, pitches) in promo_notes {
                    lane.steps[*step_index].state = StepState::Active;
                    lane.steps[*step_index].notes =
                        pitches.iter().copied().map(NoteEvent::new).collect();
                }
            }
        }

        let selected_output = project
            .chains
            .iter()
            .find_map(|chain| chain.route.output_name.clone());
        let last_saved_json = project.to_json().unwrap_or_default();

        Self {
            project,
            engine: PreviewEngine::default(),
            mode: WorkMode::Edit,
            selected_lane: if promo_capture { 4 } else { 0 },
            selected_step: 0,
            live_skips: BTreeSet::new(),
            global_live_skip: false,
            about_open: !promo_capture,
            last_frame: Instant::now(),
            splash_texture,
            midi: MidiService::new(),
            midi_ports: Vec::new(),
            selected_output,
            midi_status: MidiStatus::Disconnected,
            autosave_path,
            last_autosave: Instant::now(),
            last_saved_json,
            save_status,
            midi_input: MidiInputService::new(),
            midi_input_ports: Vec::new(),
            selected_input: None,
            midi_input_status: MidiStatus::Disconnected,
            input_mode: InputMode::Live,
            midi_thru: true,
            held_notes: BTreeSet::new(),
            capture_notes: BTreeMap::new(),
            arp_latched_notes: BTreeSet::new(),
            arp_note_order: Vec::new(),
            arp_velocities: BTreeMap::new(),
            arp_elapsed: 0.0,
            arp_cursor: 0,
            arp_random_state: 58,
            arp_running: false,
            arp_status: "ARP BEREIT".to_owned(),
            promo_capture,
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .exact_height(68.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(9, 13, 15)))
            .show(ctx, |ui| {
                ui.add_space(7.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("K.L.A.U.S. SEQUENCER 58")
                                .size(18.0)
                                .color(Color32::WHITE)
                                .strong(),
                        );
                        ui.label(
                            RichText::new("KETTEN-LOOPER FÜR ARPEGGIEN UND UNABHÄNGIGE STEPS")
                                .size(8.5)
                                .color(theme::MUTED),
                        );
                    });

                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(18.0);
                        let play_label = if self.engine.playing { "II" } else { "▶" };
                        if ui
                            .add_sized(
                                [48.0, 48.0],
                                egui::Button::new(RichText::new(play_label).size(18.0)).fill(
                                    if self.engine.playing {
                                        theme::CYAN
                                    } else {
                                        theme::PANEL_RAISED
                                    },
                                ),
                            )
                            .on_hover_text("Start / Pause")
                            .clicked()
                        {
                            self.engine.toggle();
                        }
                        if ui
                            .add_sized(
                                [44.0, 44.0],
                                egui::Button::new("■")
                                    .fill(theme::PANEL_RAISED)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(63, 76, 82))),
                            )
                            .on_hover_text("Stopp und Rückkehr zum Anfang")
                            .clicked()
                        {
                            self.engine.stop();
                            self.midi.panic();
                        }
                        ui.add(
                            egui::DragValue::new(&mut self.project.bpm)
                                .range(20.0..=300.0)
                                .speed(0.1)
                                .suffix(" BPM"),
                        );
                        ui.label(RichText::new("● INTERNAL").color(theme::CYAN).size(10.0));
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [62.0, 42.0],
                                egui::Button::new(RichText::new("PANIC").color(theme::DANGER))
                                    .fill(Color32::from_rgb(38, 22, 23))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(105, 52, 54))),
                            )
                            .on_hover_text("All Notes Off")
                            .clicked()
                        {
                            self.engine.stop();
                            self.midi.panic();
                        }
                        if ui
                            .add_sized(
                                [44.0, 42.0],
                                egui::Button::new("ⓘ")
                                    .fill(theme::PANEL_RAISED)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(63, 76, 82))),
                            )
                            .on_hover_text("Über K.L.A.U.S.")
                            .clicked()
                        {
                            self.about_open = true;
                        }
                        let perform = ui
                            .selectable_label(self.mode == WorkMode::Perform, "PERFORM")
                            .clicked();
                        let edit = ui
                            .selectable_label(self.mode == WorkMode::Edit, "EDIT")
                            .clicked();
                        if perform {
                            self.mode = WorkMode::Perform;
                        }
                        if edit {
                            self.mode = WorkMode::Edit;
                            self.live_skips.clear();
                        }
                    });
                });
            });
    }

    fn midi_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("midi_bar")
            .exact_height(76.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(12, 18, 21))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("MIDI OUT").color(theme::MUTED).size(9.0));
                    let selected_text = self
                        .selected_output
                        .as_deref()
                        .unwrap_or("Kein Ausgang gewählt");
                    egui::ComboBox::from_id_salt("midi_output")
                        .width(280.0)
                        .selected_text(selected_text)
                        .show_ui(ui, |ui| {
                            if self.midi_ports.is_empty() {
                                ui.label(
                                    RichText::new("Keine MIDI-Ausgänge gefunden")
                                        .color(theme::MUTED),
                                );
                            }
                            for port in &self.midi_ports {
                                ui.selectable_value(
                                    &mut self.selected_output,
                                    Some(port.clone()),
                                    port,
                                );
                            }
                        });

                    let connected = matches!(self.midi_status, MidiStatus::Connected(_));
                    let button_text = if connected { "TRENNEN" } else { "VERBINDEN" };
                    if ui
                        .add_enabled(
                            connected || self.selected_output.is_some(),
                            egui::Button::new(button_text).fill(theme::PANEL_RAISED),
                        )
                        .clicked()
                    {
                        if connected {
                            self.midi.disconnect();
                        } else if let Some(port) = self.selected_output.clone() {
                            for chain in &mut self.project.chains {
                                chain.route.output_name = Some(port.clone());
                            }
                            self.midi.connect(port);
                        }
                    }
                    if ui
                        .button("↻")
                        .on_hover_text("MIDI-Geräte neu einlesen")
                        .clicked()
                    {
                        self.midi.refresh_ports();
                    }

                    ui.separator();
                    let (status_text, status_color) = match &self.midi_status {
                        MidiStatus::Disconnected => ("● GETRENNT".to_owned(), theme::MUTED),
                        MidiStatus::Connecting(name) => {
                            (format!("● VERBINDE {name}"), theme::ORANGE)
                        }
                        MidiStatus::Connected(name) => {
                            (format!("● VERBUNDEN · {name}"), theme::ACID)
                        }
                        MidiStatus::Error(error) => (format!("● {error}"), theme::DANGER),
                    };
                    ui.label(RichText::new(status_text).color(status_color).size(9.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let color = if self.save_status.starts_with("AUTOSAVE-FEHLER") {
                            theme::DANGER
                        } else {
                            theme::MUTED
                        };
                        ui.label(RichText::new(&self.save_status).color(color).size(9.0));
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("MIDI IN ").color(theme::MUTED).size(9.0));
                    egui::ComboBox::from_id_salt("midi_input")
                        .width(280.0)
                        .selected_text(
                            self.selected_input
                                .as_deref()
                                .unwrap_or("Kein Eingang gewählt"),
                        )
                        .show_ui(ui, |ui| {
                            if self.midi_input_ports.is_empty() {
                                ui.label(
                                    RichText::new("Keine MIDI-Eingänge gefunden")
                                        .color(theme::MUTED),
                                );
                            }
                            for port in &self.midi_input_ports {
                                ui.selectable_value(
                                    &mut self.selected_input,
                                    Some(port.clone()),
                                    port,
                                );
                            }
                        });
                    let connected = matches!(self.midi_input_status, MidiStatus::Connected(_));
                    if ui
                        .add_enabled(
                            connected || self.selected_input.is_some(),
                            egui::Button::new(if connected { "TRENNEN" } else { "VERBINDEN" })
                                .fill(theme::PANEL_RAISED),
                        )
                        .clicked()
                    {
                        if connected {
                            self.midi_input.disconnect();
                        } else if let Some(port) = self.selected_input.clone() {
                            self.midi_input.connect(port);
                        }
                    }
                    if ui
                        .button("↻")
                        .on_hover_text("MIDI-Eingänge neu einlesen")
                        .clicked()
                    {
                        self.midi_input.refresh_ports();
                    }
                    egui::ComboBox::from_id_salt("midi_input_mode")
                        .selected_text(input_mode_label(self.input_mode))
                        .show_ui(ui, |ui| {
                            for mode in [
                                InputMode::Live,
                                InputMode::ChordCapture,
                                InputMode::StepRecord,
                                InputMode::QuantizedLive,
                            ] {
                                ui.selectable_value(
                                    &mut self.input_mode,
                                    mode,
                                    input_mode_label(mode),
                                );
                            }
                        });
                    ui.checkbox(&mut self.midi_thru, "THRU");
                    let (text, color) = midi_status_label(&self.midi_input_status);
                    ui.label(RichText::new(text).color(color).size(9.0));
                });
            });
    }

    fn autosave_if_needed(&mut self) {
        if self.last_autosave.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_autosave = Instant::now();
        self.project.sync_all_active_patterns();
        let Ok(json) = self.project.to_json() else {
            self.save_status = "AUTOSAVE-FEHLER · SERIALISIERUNG".to_owned();
            return;
        };
        if json == self.last_saved_json {
            return;
        }
        match save_project_atomic(&self.project, &self.autosave_path) {
            Ok(()) => {
                self.last_saved_json = json;
                self.save_status = "AUTOSAVE ✓".to_owned();
            }
            Err(error) => self.save_status = format!("AUTOSAVE-FEHLER · {error}"),
        }
    }

    fn poll_midi_events(&mut self) {
        while let Some(event) = self.midi.poll_event() {
            match event {
                MidiEvent::Ports(ports) => {
                    self.midi_ports = ports;
                    if self.selected_output.is_none() && self.midi_ports.len() == 1 {
                        self.selected_output = self.midi_ports.first().cloned();
                    }
                }
                MidiEvent::Status(status) => self.midi_status = status,
            }
        }
        while let Some(event) = self.midi_input.poll_event() {
            match event {
                MidiInputEvent::Ports(ports) => {
                    self.midi_input_ports = ports;
                    if self.selected_input.is_none() && self.midi_input_ports.len() == 1 {
                        self.selected_input = self.midi_input_ports.first().cloned();
                    }
                }
                MidiInputEvent::Status(status) => self.midi_input_status = status,
                MidiInputEvent::Message(message) => self.handle_midi_input(message),
            }
        }
    }

    fn handle_midi_input(&mut self, message: MidiInputMessage) {
        let decoded = decode_note(message);
        let selected_chain = self.selected_chain_index();
        let arp_enabled = selected_chain
            .and_then(|index| self.project.chains.get(index))
            .is_some_and(|chain| chain.arpeggiator.enabled);
        if self.midi_thru && message.len == 3 && (!arp_enabled || decoded.is_none()) {
            let channel = selected_chain
                .and_then(|index| self.project.chains.get(index))
                .map_or(1, |chain| chain.route.channel.clamp(1, 16));
            let forwarded = match decoded {
                Some(NoteInput::On {
                    pitch, velocity, ..
                }) => note_on(channel, pitch, velocity),
                Some(NoteInput::Off { pitch, .. }) => note_off(channel, pitch),
                None => message.data,
            };
            self.midi.schedule(Instant::now(), forwarded);
        }

        let Some(note) = decoded else {
            return;
        };
        match note {
            NoteInput::On {
                pitch, velocity, ..
            } => {
                self.held_notes.insert(pitch);
                self.arp_velocities.insert(pitch, velocity);
                if !self.arp_note_order.contains(&pitch) {
                    self.arp_note_order.push(pitch);
                }
                if arp_enabled
                    && selected_chain
                        .and_then(|index| self.project.chains.get(index))
                        .is_some_and(|chain| chain.arpeggiator.latch)
                {
                    self.arp_latched_notes.insert(pitch);
                }
                match self.input_mode {
                    InputMode::Live => {}
                    InputMode::ChordCapture | InputMode::StepRecord => {
                        self.capture_notes.insert(pitch, velocity);
                    }
                    InputMode::QuantizedLive => self.record_quantized_note(pitch, velocity),
                }
            }
            NoteInput::Off { pitch, .. } => {
                self.held_notes.remove(&pitch);
                let latch = selected_chain
                    .and_then(|index| self.project.chains.get(index))
                    .is_some_and(|chain| chain.arpeggiator.latch);
                if !latch {
                    self.arp_note_order.retain(|note| *note != pitch);
                    self.arp_velocities.remove(&pitch);
                }
                if self.held_notes.is_empty()
                    && !self.capture_notes.is_empty()
                    && matches!(
                        self.input_mode,
                        InputMode::ChordCapture | InputMode::StepRecord
                    )
                {
                    self.commit_captured_chord(self.input_mode == InputMode::StepRecord);
                }
            }
        }
    }

    fn selected_chain_index(&self) -> Option<usize> {
        self.project
            .chains
            .iter()
            .position(|chain| chain.lane_ids.contains(&self.selected_lane))
    }

    fn commit_captured_chord(&mut self, advance: bool) {
        let notes: Vec<NoteEvent> = self
            .capture_notes
            .iter()
            .map(|(pitch, velocity)| NoteEvent {
                pitch: *pitch,
                velocity: *velocity,
                gate: 0.72,
            })
            .collect();
        self.capture_notes.clear();
        let lane = &mut self.project.lanes[self.selected_lane];
        let step = &mut lane.steps[self.selected_step];
        step.notes = notes;
        step.state = StepState::Active;
        if advance {
            self.selected_step = next_timed_index(&lane.steps, self.selected_step);
        }
    }

    fn record_quantized_note(&mut self, pitch: u8, velocity: u8) {
        let step_index = if self.engine.playing && self.engine.lane_active[self.selected_lane] {
            self.engine.lane_positions[self.selected_lane]
        } else {
            self.selected_step
        };
        self.selected_step = step_index;
        let step = &mut self.project.lanes[self.selected_lane].steps[step_index];
        if let Some(existing) = step.notes.iter_mut().find(|note| note.pitch == pitch) {
            existing.velocity = velocity;
        } else {
            step.notes.push(NoteEvent {
                pitch,
                velocity,
                gate: 0.72,
            });
        }
        step.state = StepState::Active;
    }

    fn update_arpeggiator(&mut self, delta_seconds: f64) -> bool {
        let Some(chain_index) = self.selected_chain_index() else {
            self.arp_running = false;
            return false;
        };
        let chain = &self.project.chains[chain_index];
        let settings = chain.arpeggiator.clone();
        let channel = chain.route.channel.clamp(1, 16);
        if !settings.enabled {
            self.arp_elapsed = 0.0;
            self.arp_cursor = 0;
            self.arp_running = false;
            return false;
        }

        let mut active_notes = self.held_notes.clone();
        if settings.latch {
            active_notes.extend(&self.arp_latched_notes);
        }
        let sequence = build_arp_sequence(
            &active_notes,
            &self.arp_note_order,
            settings.mode,
            settings.octaves,
        );
        if sequence.is_empty() {
            self.arp_elapsed = 0.0;
            self.arp_cursor = 0;
            self.arp_running = false;
            return false;
        }

        let interval = division_seconds(self.project.bpm, settings.division);
        self.arp_elapsed += delta_seconds.max(0.0);
        let mut fired = false;
        if !self.arp_running {
            self.trigger_arpeggio_note(&sequence, channel, interval, &settings);
            self.arp_running = true;
            fired = true;
        }
        while self.arp_elapsed >= interval {
            self.arp_elapsed -= interval;
            self.trigger_arpeggio_note(&sequence, channel, interval, &settings);
            fired = true;
        }
        fired
    }

    fn trigger_arpeggio_note(
        &mut self,
        sequence: &[u8],
        channel: u8,
        interval: f64,
        settings: &crate::model::ArpeggiatorSettings,
    ) {
        let index = if settings.mode == ArpMode::Random {
            self.arp_random_state ^= self.arp_random_state >> 12;
            self.arp_random_state ^= self.arp_random_state << 25;
            self.arp_random_state ^= self.arp_random_state >> 27;
            self.arp_random_state = self.arp_random_state.wrapping_mul(0x2545_F491_4F6C_DD1D);
            self.arp_random_state as usize % sequence.len()
        } else {
            self.arp_cursor % sequence.len()
        };
        let pitch = sequence[index];
        self.arp_cursor = self.arp_cursor.wrapping_add(1);
        let velocity = self
            .arp_velocities
            .iter()
            .min_by_key(|(source_pitch, _)| source_pitch.abs_diff(pitch))
            .map_or(100, |(_, velocity)| *velocity);
        let ratchets = settings.ratchets.clamp(1, 8);
        let slice = interval / f64::from(ratchets);
        let now = Instant::now();
        for ratchet in 0..ratchets {
            let on_time = now + Duration::from_secs_f64(slice * f64::from(ratchet));
            let gate =
                (slice * f64::from(settings.gate.clamp(0.05, 1.0))).clamp(0.001, slice * 0.98);
            self.midi
                .schedule(on_time, note_on(channel, pitch, velocity));
            self.midi.schedule(
                on_time + Duration::from_secs_f64(gate),
                note_off(channel, pitch),
            );
        }
    }

    fn clear_arp_latch(&mut self) {
        self.arp_latched_notes.clear();
        self.arp_note_order
            .retain(|pitch| self.held_notes.contains(pitch));
        self.arp_velocities
            .retain(|pitch, _| self.held_notes.contains(pitch));
        self.arp_running = false;
        self.midi.panic();
        self.arp_status = "LATCH GELEERT".to_owned();
    }

    fn render_arpeggiator_pattern(&mut self, chain_index: usize) {
        let _ = self.project.sync_active_pattern_from_lanes(chain_index);
        let Some(chain) = self.project.chains.get(chain_index).cloned() else {
            return;
        };
        let mut active_notes = self.held_notes.clone();
        if chain.arpeggiator.latch {
            active_notes.extend(&self.arp_latched_notes);
        }
        let sequence = build_arp_sequence(
            &active_notes,
            &self.arp_note_order,
            chain.arpeggiator.mode,
            chain.arpeggiator.octaves,
        );
        if sequence.is_empty() {
            self.arp_status = "KEINE NOTEN ZUM RENDERN".to_owned();
            return;
        }

        let take_number = self
            .project
            .patterns
            .iter()
            .filter(|pattern| pattern.chain_id == chain.id)
            .count()
            + 1;
        let mut render_random_state = 58_u64.wrapping_add(take_number as u64);
        let mut lane_steps = vec![vec![Step::rest(); STEPS_PER_LANE]; chain.lane_ids.len()];
        for flat in 0..(chain.lane_ids.len() * STEPS_PER_LANE) {
            let lane_offset = flat / STEPS_PER_LANE;
            let step_offset = flat % STEPS_PER_LANE;
            let sequence_index = if chain.arpeggiator.mode == ArpMode::Random {
                render_random_state ^= render_random_state >> 12;
                render_random_state ^= render_random_state << 25;
                render_random_state ^= render_random_state >> 27;
                render_random_state = render_random_state.wrapping_mul(0x2545_F491_4F6C_DD1D);
                render_random_state as usize % sequence.len()
            } else {
                flat % sequence.len()
            };
            let pitch = sequence[sequence_index];
            lane_steps[lane_offset][step_offset] = Step {
                state: StepState::Active,
                notes: vec![NoteEvent {
                    pitch,
                    velocity: 100,
                    gate: chain.arpeggiator.gate,
                }],
                probability: 100,
                ratchets: chain.arpeggiator.ratchets,
            };
        }
        match self.project.add_chain_pattern(
            chain_index,
            format!("ARP TAKE {take_number:02}"),
            lane_steps,
        ) {
            Ok(_) => self.arp_status = format!("ARP TAKE {take_number:02} AKTIV"),
            Err(error) => self.arp_status = format!("RENDER-FEHLER · {error}"),
        }
    }

    fn schedule_midi(&self, triggers: &[StepTrigger]) {
        let any_solo = self.project.lanes.iter().any(|lane| lane.solo);
        let base_time = Instant::now();

        for trigger in triggers {
            let Some(lane) = self.project.lanes.get(trigger.lane) else {
                continue;
            };
            if lane.muted || (any_solo && !lane.solo) {
                continue;
            }
            if self.live_skips.contains(&(trigger.lane, trigger.step)) {
                continue;
            }
            let Some(step) = lane.steps.get(trigger.step) else {
                continue;
            };
            if step.state != StepState::Active || step.notes.is_empty() {
                continue;
            }
            if !probability_hits(
                58,
                trigger.lane,
                trigger.step,
                trigger.cycle,
                step.probability,
            ) {
                continue;
            }

            let Some(chain) = self.project.chains.get(trigger.chain) else {
                continue;
            };
            let channel = chain.route.channel.clamp(1, 16);
            let step_seconds = division_seconds(self.project.bpm, chain.division);
            let ratchets = step.ratchets.clamp(1, 8);
            let slice_seconds = step_seconds / f64::from(ratchets);
            for ratchet in 0..ratchets {
                let offset = slice_seconds * f64::from(ratchet);
                let note_time = base_time + Duration::from_secs_f64(offset);
                for note in &step.notes {
                    let gate_seconds = (slice_seconds * f64::from(note.gate.clamp(0.05, 1.0)))
                        .clamp(0.001, slice_seconds * 0.98);
                    self.midi
                        .schedule(note_time, note_on(channel, note.pitch, note.velocity));
                    self.midi.schedule(
                        note_time + Duration::from_secs_f64(gate_seconds),
                        note_off(channel, note.pitch),
                    );
                }
            }
        }
    }

    fn inspector(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("step_inspector")
            .exact_width(246.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(14.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.heading(format!("STEP {:02}", self.selected_step + 1));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new("ACTIVE").color(theme::CYAN).size(10.0));
                            });
                        });
                        ui.separator();

                        let lane = &mut self.project.lanes[self.selected_lane];
                        let default_pitch = lane.default_pitch();
                        let step = &mut lane.steps[self.selected_step];
                        ui.label(
                            RichText::new(&lane.name).color(theme::lane_color(self.selected_lane)),
                        );
                        ui.add_space(6.0);
                        ui.label(RichText::new("PROBABILITY").color(theme::MUTED).size(9.0));
                        ui.add(egui::Slider::new(&mut step.probability, 0..=100).suffix(" %"));
                        ui.label(RichText::new("RATCHETS").color(theme::MUTED).size(9.0));
                        ui.add(egui::Slider::new(&mut step.ratchets, 1..=8));

                        let mut velocity = step.notes.first().map_or(100, |note| note.velocity);
                        ui.label(RichText::new("VELOCITY").color(theme::MUTED).size(9.0));
                        if ui.add(egui::Slider::new(&mut velocity, 1..=127)).changed() {
                            for note in &mut step.notes {
                                note.velocity = velocity;
                            }
                        }
                        let mut gate = step.notes.first().map_or(0.72, |note| note.gate);
                        ui.label(RichText::new("GATE").color(theme::MUTED).size(9.0));
                        if ui
                            .add(egui::Slider::new(&mut gate, 0.05..=1.0).suffix(" ×"))
                            .changed()
                        {
                            for note in &mut step.notes {
                                note.gate = gate;
                            }
                        }

                        ui.add_space(10.0);
                        ui.label(RichText::new("STEP-STATUS").color(theme::MUTED).size(9.0));
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(step.state == StepState::Active, "ACTIVE")
                                .clicked()
                            {
                                step.state = StepState::Active;
                                if step.notes.is_empty() {
                                    step.notes.push(crate::model::NoteEvent::new(default_pitch));
                                }
                            }
                            if ui
                                .selectable_label(step.state == StepState::Rest, "REST")
                                .clicked()
                            {
                                step.state = StepState::Rest;
                            }
                            if ui
                                .selectable_label(step.state == StepState::Skip, "SKIP")
                                .clicked()
                            {
                                step.state = StepState::Skip;
                            }
                        });

                        ui.separator();
                        ui.label(RichText::new("KETTE").color(theme::MUTED).size(9.0));
                        let mut clear_arp_latch = false;
                        let mut render_arp = false;
                        let mut switch_pattern = None;
                        if let Some(chain_index) = self
                            .project
                            .chains
                            .iter()
                            .position(|chain| chain.lane_ids.contains(&self.selected_lane))
                        {
                            let active_pattern_name = self
                                .project
                                .active_pattern_by_chain
                                .get(chain_index)
                                .and_then(|index| self.project.patterns.get(*index))
                                .map_or("OHNE PATTERN", |pattern| pattern.name.as_str())
                                .to_owned();
                            let pattern_count = self
                                .project
                                .patterns
                                .iter()
                                .filter(|pattern| {
                                    pattern.chain_id == self.project.chains[chain_index].id
                                })
                                .count();
                            let pattern_choices: Vec<(usize, String)> = self
                                .project
                                .patterns
                                .iter()
                                .enumerate()
                                .filter(|(_, pattern)| {
                                    pattern.chain_id == self.project.chains[chain_index].id
                                })
                                .map(|(index, pattern)| (index, pattern.name.clone()))
                                .collect();
                            let active_pattern = self
                                .project
                                .active_pattern_by_chain
                                .get(chain_index)
                                .copied();
                            let chain = &mut self.project.chains[chain_index];
                            ui.label(RichText::new(&chain.name).strong());
                            ui.label(format!(
                                "{} Steps · {} · {} Patterns",
                                chain.lane_ids.len() * 16,
                                active_pattern_name,
                                pattern_count
                            ));
                            egui::ComboBox::from_id_salt("chain_pattern")
                                .selected_text(&active_pattern_name)
                                .show_ui(ui, |ui| {
                                    for (pattern_index, pattern_name) in &pattern_choices {
                                        if ui
                                            .selectable_label(
                                                active_pattern == Some(*pattern_index),
                                                pattern_name,
                                            )
                                            .clicked()
                                        {
                                            switch_pattern = Some(*pattern_index);
                                        }
                                    }
                                });

                            ui.horizontal(|ui| {
                                ui.label(RichText::new("MIDI-KANAL").color(theme::MUTED).size(9.0));
                                ui.add(
                                    egui::DragValue::new(&mut chain.route.channel)
                                        .range(1..=16)
                                        .speed(0.2),
                                );
                            });
                            ui.label(RichText::new("RICHTUNG").color(theme::MUTED).size(9.0));
                            egui::ComboBox::from_id_salt("chain_direction")
                                .selected_text(direction_label(chain.direction))
                                .show_ui(ui, |ui| {
                                    for direction in [
                                        Direction::Forward,
                                        Direction::Reverse,
                                        Direction::PingPong,
                                        Direction::Random,
                                    ] {
                                        ui.selectable_value(
                                            &mut chain.direction,
                                            direction,
                                            direction_label(direction),
                                        );
                                    }
                                });

                            ui.label(RichText::new("TEILUNG").color(theme::MUTED).size(9.0));
                            egui::ComboBox::from_id_salt("chain_division")
                                .selected_text(division_label(chain.division))
                                .show_ui(ui, |ui| {
                                    for division in [
                                        StepDivision::Quarter,
                                        StepDivision::Eighth,
                                        StepDivision::EighthTriplet,
                                        StepDivision::Sixteenth,
                                        StepDivision::SixteenthTriplet,
                                        StepDivision::ThirtySecond,
                                    ] {
                                        ui.selectable_value(
                                            &mut chain.division,
                                            division,
                                            division_label(division),
                                        );
                                    }
                                });

                            ui.label(RichText::new("SWING").color(theme::MUTED).size(9.0));
                            let mut swing_percent = chain.swing * 100.0;
                            if ui
                                .add(
                                    egui::Slider::new(&mut swing_percent, 50.0..=75.0).suffix(" %"),
                                )
                                .changed()
                            {
                                chain.swing = swing_percent / 100.0;
                            }

                            ui.separator();
                            ui.label(RichText::new("ARPEGGIATOR").color(theme::ORANGE).size(10.0));
                            ui.checkbox(&mut chain.arpeggiator.enabled, "LIVE ARP AKTIV");
                            if chain.arpeggiator.enabled {
                                egui::ComboBox::from_id_salt("arp_mode")
                                    .selected_text(arp_mode_label(chain.arpeggiator.mode))
                                    .show_ui(ui, |ui| {
                                        for mode in [
                                            ArpMode::Up,
                                            ArpMode::Down,
                                            ArpMode::UpDown,
                                            ArpMode::Random,
                                            ArpMode::AsPlayed,
                                        ] {
                                            ui.selectable_value(
                                                &mut chain.arpeggiator.mode,
                                                mode,
                                                arp_mode_label(mode),
                                            );
                                        }
                                    });
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut chain.arpeggiator.latch, "LATCH");
                                    if ui.button("LATCH LEEREN").clicked() {
                                        clear_arp_latch = true;
                                    }
                                });
                                ui.label(RichText::new("OKTAVEN").color(theme::MUTED).size(9.0));
                                ui.add(egui::Slider::new(&mut chain.arpeggiator.octaves, 1..=4));
                                ui.label(RichText::new("ARP-RATE").color(theme::MUTED).size(9.0));
                                egui::ComboBox::from_id_salt("arp_division")
                                    .selected_text(division_label(chain.arpeggiator.division))
                                    .show_ui(ui, |ui| {
                                        for division in [
                                            StepDivision::Quarter,
                                            StepDivision::Eighth,
                                            StepDivision::EighthTriplet,
                                            StepDivision::Sixteenth,
                                            StepDivision::SixteenthTriplet,
                                            StepDivision::ThirtySecond,
                                        ] {
                                            ui.selectable_value(
                                                &mut chain.arpeggiator.division,
                                                division,
                                                division_label(division),
                                            );
                                        }
                                    });
                                ui.label(
                                    RichText::new("ARP-RATCHETS").color(theme::MUTED).size(9.0),
                                );
                                ui.add(egui::Slider::new(&mut chain.arpeggiator.ratchets, 1..=8));
                                ui.label(RichText::new("ARP-GATE").color(theme::MUTED).size(9.0));
                                ui.add(
                                    egui::Slider::new(&mut chain.arpeggiator.gate, 0.05..=1.0)
                                        .suffix(" ×"),
                                );
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 36.0],
                                        egui::Button::new("ARP → NEUES PATTERN")
                                            .fill(Color32::from_rgb(54, 33, 70)),
                                    )
                                    .clicked()
                                {
                                    render_arp = true;
                                }
                                ui.label(
                                    RichText::new(&self.arp_status)
                                        .color(theme::MUTED)
                                        .size(9.0),
                                );
                            }
                        }

                        if clear_arp_latch {
                            self.clear_arp_latch();
                        }
                        if render_arp && let Some(chain_index) = self.selected_chain_index() {
                            self.render_arpeggiator_pattern(chain_index);
                        }
                        if let Some(pattern_index) = switch_pattern
                            && let Some(chain_index) = self.selected_chain_index()
                        {
                            let _ = self.project.sync_active_pattern_from_lanes(chain_index);
                            if let Err(error) =
                                self.project.apply_chain_pattern(chain_index, pattern_index)
                            {
                                self.arp_status = format!("PATTERN-FEHLER · {error}");
                            } else {
                                self.engine.stop();
                                self.midi.panic();
                                self.live_skips.clear();
                            }
                        }

                        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                            ui.label(
                                RichText::new("Klaus hat die Clock.")
                                    .italics()
                                    .color(theme::MUTED),
                            );
                        });
                    });
            });
    }

    fn piano_roll(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("piano_roll")
            .exact_height(if self.promo_capture { 220.0 } else { 286.0 })
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(11, 15, 17))
                    .inner_margin(10.0),
            )
            .show(ctx, |ui| {
                let lane_kind = self.project.lanes[self.selected_lane].kind;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if lane_kind == LaneKind::Drums {
                            "DRUM PIANO ROLL"
                        } else {
                            "PIANO ROLL"
                        })
                        .strong(),
                    );
                    ui.label(RichText::new("D DORIAN · SCALE FOLD").color(theme::MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("GETEILT · FULLSCREEN SPÄTER")
                                .size(9.0)
                                .color(theme::MUTED),
                        );
                    });
                });
                ui.separator();

                let base_pitch = if lane_kind == LaneKind::Drums { 36 } else { 48 };
                let row_count = if lane_kind == LaneKind::Drums { 8 } else { 12 };
                let cell_width =
                    ((ui.available_width() - 72.0) / STEPS_PER_LANE as f32 - 3.0).clamp(22.0, 44.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for row in (0..row_count).rev() {
                        let pitch = base_pitch + row as u8;
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [62.0, 24.0],
                                egui::Label::new(
                                    RichText::new(note_name(pitch, lane_kind)).color(theme::MUTED),
                                ),
                            );
                            for step_index in 0..STEPS_PER_LANE {
                                let has_note = self.project.lanes[self.selected_lane].steps
                                    [step_index]
                                    .notes
                                    .iter()
                                    .any(|note| note.pitch == pitch);
                                let selected = step_index == self.selected_step;
                                let fill = if has_note {
                                    theme::lane_color(self.selected_lane)
                                } else if selected {
                                    Color32::from_rgb(38, 49, 53)
                                } else {
                                    Color32::from_rgb(20, 27, 30)
                                };
                                let response = ui.add_sized(
                                    [cell_width, 24.0],
                                    egui::Button::new("").fill(fill).stroke(if selected {
                                        Stroke::new(1.0, Color32::WHITE)
                                    } else {
                                        Stroke::new(1.0, Color32::from_rgb(46, 57, 62))
                                    }),
                                );
                                if response.clicked() {
                                    self.selected_step = step_index;
                                    self.project.lanes[self.selected_lane].steps[step_index]
                                        .toggle_note(pitch);
                                }
                            }
                        });
                    }
                });
            });
    }

    fn matrix(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::GRAPHITE).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CHAIN MATRIX").color(theme::MUTED).size(9.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.mode == WorkMode::Perform {
                            ui.checkbox(&mut self.global_live_skip, "GLOBAL LIVE-SKIP");
                            if ui.button("LIVE-SKIPS LÖSCHEN").clicked() {
                                self.live_skips.clear();
                            }
                        } else {
                            ui.label(RichText::new("EDIT MODE").color(theme::CYAN).size(9.0));
                        }
                    });
                });
                ui.add_space(4.0);

                for lane_index in 0..self.project.lanes.len() {
                    let color = theme::lane_color(lane_index);
                    let lane_name = self.project.lanes[lane_index].name.clone();
                    let lane_kind = self.project.lanes[lane_index].kind;
                    ui.horizontal(|ui| {
                        let lane_selected = lane_index == self.selected_lane;
                        if ui
                            .add_sized(
                                [112.0, 42.0],
                                egui::Button::new(
                                    RichText::new(format!("{}  {}", lane_index + 1, lane_name))
                                        .color(if lane_selected { Color32::BLACK } else { color })
                                        .size(10.0),
                                )
                                .fill(if lane_selected {
                                    color
                                } else {
                                    theme::PANEL
                                }),
                            )
                            .clicked()
                        {
                            self.selected_lane = lane_index;
                        }

                        let reserved = 66.0;
                        let step_size =
                            ((ui.available_width() - reserved) / 16.0 - 4.0).clamp(25.0, 46.0);
                        for step_index in 0..STEPS_PER_LANE {
                            let step = &self.project.lanes[lane_index].steps[step_index];
                            let live_skipped = self.live_skips.contains(&(lane_index, step_index));
                            let is_current = self.engine.playing
                                && self.engine.lane_active[lane_index]
                                && self.engine.lane_positions[lane_index] == step_index;
                            let selected = lane_index == self.selected_lane
                                && step_index == self.selected_step;
                            let label = if live_skipped {
                                "L".to_owned()
                            } else if step.state == StepState::Skip {
                                "×".to_owned()
                            } else if step.ratchets > 1 {
                                format!("{}", step.ratchets)
                            } else if step.notes.len() > 1 {
                                format!("{}n", step.notes.len())
                            } else {
                                String::new()
                            };
                            let fill = if live_skipped {
                                theme::DANGER.gamma_multiply(0.42)
                            } else {
                                match step.state {
                                    StepState::Active => color
                                        .gamma_multiply(0.38 + f32::from(step.probability) / 180.0),
                                    StepState::Rest => Color32::from_rgb(18, 24, 27),
                                    StepState::Skip => Color32::from_rgb(31, 25, 27),
                                }
                            };
                            let stroke = if is_current {
                                Stroke::new(2.5, Color32::WHITE)
                            } else if selected {
                                Stroke::new(2.0, theme::ACID)
                            } else {
                                Stroke::new(1.0, Color32::from_rgb(49, 61, 66))
                            };
                            let response = ui.add_sized(
                                [step_size, 42.0],
                                egui::Button::new(RichText::new(label).size(10.0))
                                    .fill(fill)
                                    .stroke(stroke),
                            );
                            if response.clicked() {
                                self.selected_lane = lane_index;
                                self.selected_step = step_index;
                                match self.mode {
                                    WorkMode::Edit => {
                                        let default_pitch = if lane_kind == LaneKind::Melody {
                                            48 + lane_index as u8
                                        } else {
                                            36 + (lane_index.saturating_sub(6) as u8) * 2
                                        };
                                        self.project.lanes[lane_index].steps[step_index]
                                            .toggle_active(default_pitch);
                                    }
                                    WorkMode::Perform => {
                                        if self.global_live_skip {
                                            let should_add = !self
                                                .live_skips
                                                .contains(&(lane_index, step_index));
                                            for target_lane in 0..self.project.lanes.len() {
                                                if should_add {
                                                    self.live_skips
                                                        .insert((target_lane, step_index));
                                                } else {
                                                    self.live_skips
                                                        .remove(&(target_lane, step_index));
                                                }
                                            }
                                        } else if !self.live_skips.remove(&(lane_index, step_index))
                                        {
                                            self.live_skips.insert((lane_index, step_index));
                                        }
                                    }
                                }
                            }
                            if response.secondary_clicked() && self.mode == WorkMode::Edit {
                                self.project.lanes[lane_index].steps[step_index].state =
                                    StepState::Skip;
                            }
                        }

                        let lane = &mut self.project.lanes[lane_index];
                        let mute_fill = if lane.muted {
                            color
                        } else {
                            theme::PANEL_RAISED
                        };
                        if ui
                            .add_sized(
                                [28.0, 32.0],
                                egui::Button::new("M")
                                    .fill(mute_fill)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(63, 76, 82))),
                            )
                            .clicked()
                        {
                            lane.muted = !lane.muted;
                        }
                        let solo_fill = if lane.solo {
                            theme::ACID
                        } else {
                            theme::PANEL_RAISED
                        };
                        if ui
                            .add_sized(
                                [28.0, 32.0],
                                egui::Button::new("S")
                                    .fill(solo_fill)
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(63, 76, 82))),
                            )
                            .clicked()
                        {
                            lane.solo = !lane.solo;
                        }
                    });
                    ui.add_space(3.0);
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(match self.mode {
                            WorkMode::Edit => "Tippen: Step aktivieren · Rechtsklick: Skip",
                            WorkMode::Perform => "Tippen: temporärer Live-Skip",
                        })
                        .color(theme::MUTED)
                        .size(9.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("SCENE 04 · NIGHT DRIVE")
                                .color(theme::ACID)
                                .size(9.0),
                        );
                    });
                });
            });
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        let mut open = self.about_open;
        let texture = &self.splash_texture;
        egui::Window::new("K.L.A.U.S. SEQUENCER 58")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(9, 13, 15)))
            .show(ctx, |ui| {
                let width = 720.0_f32.min(ui.ctx().screen_rect().width() * 0.72);
                let size = egui::vec2(width, width / 16.0 * 9.0);
                ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.heading(RichText::new("K.L.A.U.S. Sequencer 58").color(Color32::WHITE));
                        ui.label(
                            RichText::new("Ketten-Looper für Arpeggien und unabhängige Steps")
                                .color(theme::MUTED),
                        );
                        ui.label(
                            RichText::new("Cosmic sequencing from somewhere near Berlin")
                                .color(theme::CYAN)
                                .italics(),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("v0.4.0 · PROTOTYPE").color(theme::MUTED));
                    });
                });
            });
        self.about_open = open;
    }
}

impl eframe::App for KlausApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_midi_events();
        self.autosave_if_needed();
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        if self.update_arpeggiator(delta) {
            ctx.request_repaint();
        }
        if self.arp_running {
            ctx.request_repaint_after(Duration::from_millis(2));
        }
        let triggers = self.engine.update(&self.project, &self.live_skips, delta);
        if !triggers.is_empty() {
            self.schedule_midi(&triggers);
            ctx.request_repaint();
        }
        if self.engine.playing {
            ctx.request_repaint();
        }

        self.top_bar(ctx);
        self.midi_bar(ctx);
        self.piano_roll(ctx);
        self.inspector(ctx);
        self.matrix(ctx);
        self.about_window(ctx);
    }
}

impl Drop for KlausApp {
    fn drop(&mut self) {
        self.project.sync_all_active_patterns();
        let _ = save_project_atomic(&self.project, &self.autosave_path);
    }
}

fn note_name(pitch: u8, kind: LaneKind) -> String {
    if kind == LaneKind::Drums {
        return match pitch {
            36 => "KICK".to_owned(),
            37 => "RIM".to_owned(),
            38 => "SNARE".to_owned(),
            39 => "CLAP".to_owned(),
            40 => "SNARE 2".to_owned(),
            41 => "TOM".to_owned(),
            42 => "CLOSED HH".to_owned(),
            43 => "TOM 2".to_owned(),
            _ => format!("MIDI {pitch}"),
        };
    }
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let octave = i16::from(pitch) / 12 - 1;
    format!("{}{}", NAMES[pitch as usize % 12], octave)
}

fn direction_label(direction: Direction) -> &'static str {
    match direction {
        Direction::Forward => "VORWÄRTS",
        Direction::Reverse => "RÜCKWÄRTS",
        Direction::PingPong => "PING-PONG",
        Direction::Random => "ZUFALL",
    }
}

fn division_label(division: StepDivision) -> &'static str {
    match division {
        StepDivision::Quarter => "1/4",
        StepDivision::Eighth => "1/8",
        StepDivision::EighthTriplet => "1/8T",
        StepDivision::Sixteenth => "1/16",
        StepDivision::SixteenthTriplet => "1/16T",
        StepDivision::ThirtySecond => "1/32",
    }
}

fn input_mode_label(mode: InputMode) -> &'static str {
    match mode {
        InputMode::Live => "LIVE-INSTRUMENT",
        InputMode::ChordCapture => "AKKORD CAPTURE",
        InputMode::StepRecord => "STEP RECORD",
        InputMode::QuantizedLive => "LIVE RECORD · QUANTISIERT",
    }
}

fn arp_mode_label(mode: ArpMode) -> &'static str {
    match mode {
        ArpMode::Up => "AUFWÄRTS",
        ArpMode::Down => "ABWÄRTS",
        ArpMode::UpDown => "AUF + AB",
        ArpMode::Random => "ZUFALL",
        ArpMode::AsPlayed => "SPIELREIHENFOLGE",
    }
}

fn build_arp_sequence(
    active_notes: &BTreeSet<u8>,
    played_order: &[u8],
    mode: ArpMode,
    octaves: u8,
) -> Vec<u8> {
    let mut base: Vec<u8> = if mode == ArpMode::AsPlayed {
        played_order
            .iter()
            .copied()
            .filter(|pitch| active_notes.contains(pitch))
            .collect()
    } else {
        active_notes.iter().copied().collect()
    };
    for pitch in active_notes {
        if !base.contains(pitch) {
            base.push(*pitch);
        }
    }
    if base.is_empty() {
        return Vec::new();
    }
    if mode != ArpMode::AsPlayed {
        base.sort_unstable();
    }

    let mut expanded = Vec::new();
    for octave in 0..octaves.clamp(1, 4) {
        for pitch in &base {
            let transposed = u16::from(*pitch) + u16::from(octave) * 12;
            if transposed <= 127 {
                expanded.push(transposed as u8);
            }
        }
    }
    match mode {
        ArpMode::Down => expanded.reverse(),
        ArpMode::UpDown if expanded.len() > 2 => {
            let return_path: Vec<u8> = expanded[1..expanded.len() - 1]
                .iter()
                .rev()
                .copied()
                .collect();
            expanded.extend(return_path);
        }
        ArpMode::Up | ArpMode::Random | ArpMode::AsPlayed | ArpMode::UpDown => {}
    }
    expanded
}

fn midi_status_label(status: &MidiStatus) -> (String, Color32) {
    match status {
        MidiStatus::Disconnected => ("● GETRENNT".to_owned(), theme::MUTED),
        MidiStatus::Connecting(name) => (format!("● VERBINDE {name}"), theme::ORANGE),
        MidiStatus::Connected(name) => (format!("● VERBUNDEN · {name}"), theme::ACID),
        MidiStatus::Error(error) => (format!("● {error}"), theme::DANGER),
    }
}

#[allow(dead_code)]
fn touch_target(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, 6.0, theme::PANEL_RAISED);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::build_arp_sequence;
    use crate::model::ArpMode;

    #[test]
    fn arpeggio_up_down_avoids_duplicate_turning_notes() {
        let notes = BTreeSet::from([60, 64, 67]);
        assert_eq!(
            build_arp_sequence(&notes, &[], ArpMode::UpDown, 1),
            vec![60, 64, 67, 64]
        );
    }

    #[test]
    fn arpeggio_expands_octaves_without_exceeding_midi_range() {
        let notes = BTreeSet::from([120]);
        assert_eq!(build_arp_sequence(&notes, &[], ArpMode::Up, 4), vec![120]);
    }

    #[test]
    fn as_played_preserves_input_order() {
        let notes = BTreeSet::from([60, 64, 67]);
        assert_eq!(
            build_arp_sequence(&notes, &[67, 60, 64], ArpMode::AsPlayed, 1),
            vec![67, 60, 64]
        );
    }
}
