use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, TextureHandle};

use crate::engine::{
    PreviewEngine, StepTrigger, division_seconds, next_timed_index, probability_hits,
};
use crate::generator::{GenerationMode, generate_lane};
use crate::midi::{
    MidiEvent, MidiInputEvent, MidiInputMessage, MidiInputService, MidiService, MidiStatus,
    NoteInput, decode_note, note_off, note_on, sound_selection_messages,
};
use crate::midi_export::export_song;
use crate::model::{
    ArpMode, ClipTransition, Direction, GeneratorRole, LaneGeneratorConfig, LaneKind,
    MidiClockMode, MusicalScale, NoteEvent, PerformanceAction, PerformanceEvent, PerformanceLoop,
    Project, STEPS_PER_LANE, SceneLaunchQuantization, SongClipSource, Step, StepDivision,
    StepState,
};
use crate::persistence::{default_autosave_path, load_project, save_project_atomic};
use crate::theme;

const PIANO_ROLL_MIN_HEIGHT: f32 = 150.0;
const PIANO_ROLL_DEFAULT_HEIGHT: f32 = 320.0;
const MATRIX_MIN_HEIGHT: f32 = 160.0;

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

#[derive(Debug, Clone)]
struct GeneratorPreview {
    lane: usize,
    steps: Vec<Step>,
    seed: u64,
    mode: GenerationMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratorAction {
    Generate,
    Variant,
    Mutate,
    Apply,
    Discard,
    ResetProfile,
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
    queued_scene: Option<usize>,
    scene_status: String,
    transport_pulses: u64,
    clock_accumulator: f64,
    external_clock_pulses: u64,
    last_external_clock: Option<Instant>,
    clock_status: String,
    project_path: Option<PathBuf>,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
    history_project: Project,
    generator_open: bool,
    generator_preview: Option<GeneratorPreview>,
    generator_status: String,
    sound_status: String,
    performance_window_open: bool,
    performance_capture_active: bool,
    performance_playback_active: bool,
    performance_playback_cursor: usize,
    performance_loop_pass: u8,
    selected_performance_event: Option<usize>,
    performance_status: String,
    composer_open: bool,
    composer_fullscreen: bool,
    piano_roll_window_open: bool,
    piano_roll_fullscreen: bool,
    piano_roll_restore_rect: Option<egui::Rect>,
    selected_song_clip: Option<usize>,
    song_playback_active: bool,
    song_clip_index: usize,
    song_transpose: i8,
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
        let (mut project, mut save_status) = if autosave_path.exists() {
            match load_project(&autosave_path) {
                Ok(project) => (project, "AUTOSAVE WIEDERHERGESTELLT".to_owned()),
                Err(error) => (Project::demo(), format!("AUTOSAVE-FEHLER · {error}")),
            }
        } else {
            (Project::demo(), "AUTOSAVE BEREIT".to_owned())
        };
        let promo_capture = std::env::var_os("KLAUS_PROMO_CAPTURE").is_some();
        let timeline_capture = std::env::var_os("KLAUS_TIMELINE_CAPTURE").is_some();
        let composer_capture = std::env::var_os("KLAUS_COMPOSER_CAPTURE").is_some();
        if promo_capture || timeline_capture || composer_capture {
            project = Project::demo();
            save_status = "DEMO-CAPTURE".to_owned();
        }
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
            if let Some(chain) = project
                .chains
                .iter_mut()
                .find(|chain| chain.lane_ids.contains(&4))
            {
                chain.route.sound.enabled = true;
                chain.route.sound.bank_msb = Some(87);
                chain.route.sound.bank_lsb = Some(0);
                chain.route.sound.program = 17;
            }
        }
        if timeline_capture {
            let scene_id = project.scenes.first().map_or(0, |scene| scene.id);
            project.performance_timeline = vec![
                PerformanceEvent {
                    pulse: 0,
                    action: PerformanceAction::TransportStart,
                },
                PerformanceEvent {
                    pulse: 48,
                    action: PerformanceAction::LiveSkip {
                        lane: 0,
                        step: 11,
                        enabled: true,
                    },
                },
                PerformanceEvent {
                    pulse: 96,
                    action: PerformanceAction::SceneLaunch { scene_id },
                },
                PerformanceEvent {
                    pulse: 144,
                    action: PerformanceAction::LiveSkip {
                        lane: 0,
                        step: 11,
                        enabled: false,
                    },
                },
                PerformanceEvent {
                    pulse: 192,
                    action: PerformanceAction::TransportStop,
                },
            ];
            project.performance_takes[0].name = "BERLIN NACHT".to_owned();
            project.performance_takes[0].loop_block = Some(PerformanceLoop {
                start_pulse: 48,
                end_pulse: 192,
                passes: 4,
            });
            project.sync_active_performance_take();
            project.add_performance_take(true);
            project.performance_takes[1].name = "BERLIN NACHT · VARIANTE".to_owned();
            project.select_performance_take(0);
        }
        if composer_capture {
            let take_id = project.performance_takes[0].id;
            let scene_id = project.scenes[0].id;
            let first = project.add_song_clip(SongClipSource::Scene { scene_id });
            project.song_clips[first].name = "INTRO".to_owned();
            project.song_clips[first].bars = 4;
            let second = project.add_song_clip(SongClipSource::Take { take_id });
            project.song_clips[second].name = "PULS".to_owned();
            project.song_clips[second].bars = 8;
            project.song_clips[second].repeats = 2;
            project.song_clips[second].transpose = 7;
            let third = project.add_song_clip(SongClipSource::Scene { scene_id });
            project.song_clips[third].name = "FINALE".to_owned();
            project.song_clips[third].bars = 4;
        }

        let selected_output = project
            .chains
            .iter()
            .find_map(|chain| chain.route.output_name.clone());
        let last_saved_json = project.to_json().unwrap_or_default();
        let history_project = project.clone();
        let selected_lane = if promo_capture { 4 } else { 0 };
        let generator_preview = promo_capture.then(|| GeneratorPreview {
            lane: selected_lane,
            steps: generate_lane(
                &project.generation_plan,
                selected_lane,
                project.lanes[selected_lane].kind,
                &project.lanes[selected_lane].steps,
                GenerationMode::Generate,
            ),
            seed: project.generation_plan.seed,
            mode: GenerationMode::Generate,
        });

        Self {
            project,
            engine: PreviewEngine::default(),
            mode: WorkMode::Edit,
            selected_lane,
            selected_step: 0,
            live_skips: BTreeSet::new(),
            global_live_skip: false,
            about_open: !promo_capture && !timeline_capture && !composer_capture,
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
            queued_scene: None,
            scene_status: "SCENE 01 AKTIV".to_owned(),
            transport_pulses: 0,
            clock_accumulator: 0.0,
            external_clock_pulses: 0,
            last_external_clock: None,
            clock_status: "CLOCK INTERN".to_owned(),
            project_path: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            history_project,
            generator_open: promo_capture,
            generator_preview,
            generator_status: if promo_capture {
                "PROMO-VORSCHAU · SEED 58".to_owned()
            } else {
                "KLAUS WARTET AUF EIN MOTIV".to_owned()
            },
            sound_status: "SOUND-ZIEL BEREIT".to_owned(),
            performance_window_open: timeline_capture,
            performance_capture_active: false,
            performance_playback_active: false,
            performance_playback_cursor: 0,
            performance_loop_pass: 1,
            selected_performance_event: None,
            performance_status: "PERFORMANCE-TAKE BEREIT".to_owned(),
            composer_open: composer_capture,
            composer_fullscreen: false,
            piano_roll_window_open: false,
            piano_roll_fullscreen: false,
            piano_roll_restore_rect: None,
            selected_song_clip: composer_capture.then_some(1),
            song_playback_active: false,
            song_clip_index: 0,
            song_transpose: 0,
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar")
            .min_height(68.0)
            .frame(egui::Frame::new().fill(Color32::from_rgb(9, 13, 15)))
            .show(ctx, |ui| {
                ui.add_space(7.0);
                ui.horizontal_wrapped(|ui| {
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
                            self.toggle_transport();
                        }
                        if ui
                            .add_sized(
                                [44.0, 44.0],
                                egui::Button::new("■")
                                    .fill(theme::PANEL_RAISED)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(63, 76, 82))),
                            )
                            .on_hover_text("Stopp und Rückkehr zum Anfang")
                            .clicked()
                        {
                            self.stop_transport();
                        }
                        ui.add_enabled(
                            self.project.midi_clock_mode != MidiClockMode::Follower,
                            egui::DragValue::new(&mut self.project.bpm)
                                .range(20.0..=300.0)
                                .speed(0.1)
                                .suffix(" BPM"),
                        );
                        ui.label(
                            RichText::new(clock_mode_label(self.project.midi_clock_mode))
                                .color(theme::CYAN)
                                .size(10.0),
                        );
                        if self.performance_capture_active {
                            ui.label(
                                RichText::new("● REC")
                                    .color(theme::DANGER)
                                    .strong()
                                    .size(11.0),
                            );
                        } else if self.performance_playback_active {
                            ui.label(
                                RichText::new("▶ TAKE")
                                    .color(theme::ACID)
                                    .strong()
                                    .size(11.0),
                            );
                        }
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [62.0, 42.0],
                                egui::Button::new(RichText::new("PANIC").color(theme::DANGER))
                                    .fill(Color32::from_rgb(38, 22, 23))
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(105, 52, 54))),
                            )
                            .on_hover_text("All Notes Off")
                            .clicked()
                        {
                            self.stop_transport();
                        }
                        if ui
                            .add_sized(
                                [44.0, 42.0],
                                egui::Button::new("ⓘ")
                                    .fill(theme::PANEL_RAISED)
                                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(63, 76, 82))),
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
                            self.clear_live_skips_recorded();
                        }
                    });
                });
                if ui.min_rect().bottom() > ui.clip_rect().bottom() {
                    ui.ctx().request_discard("toolbar height changed");
                }
            });
    }

    fn project_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("project_bar")
            .min_height(50.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(10, 15, 18))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("ÖFFNEN").clicked() {
                        self.open_project();
                    }
                    if ui.button("SPEICHERN").clicked() {
                        self.save_project();
                    }
                    if ui.button("SPEICHERN UNTER").clicked() {
                        self.save_project_as();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(!self.undo_stack.is_empty(), egui::Button::new("↶ UNDO"))
                        .clicked()
                    {
                        self.undo();
                    }
                    if ui
                        .add_enabled(!self.redo_stack.is_empty(), egui::Button::new("↷ REDO"))
                        .clicked()
                    {
                        self.redo();
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("✦ KOSMISCHER GENERATOR").color(theme::ACID),
                            )
                            .fill(theme::PANEL_RAISED)
                            .stroke(Stroke::new(1.0_f32, theme::ACID.gamma_multiply(0.7))),
                        )
                        .on_hover_text(
                            "Berlin-School-inspirierte Motive für die gewählte Reihe erzeugen",
                        )
                        .clicked()
                    {
                        self.generator_open = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("● PERFORMANCE CAPTURE").color(
                                if self.performance_capture_active {
                                    theme::DANGER
                                } else {
                                    theme::CYAN
                                },
                            ))
                            .fill(theme::PANEL_RAISED)
                            .stroke(Stroke::new(1.0_f32, theme::CYAN.gamma_multiply(0.7))),
                        )
                        .clicked()
                    {
                        self.performance_window_open = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new("▤ COMPOSER").color(theme::ORANGE))
                                .fill(theme::PANEL_RAISED)
                                .stroke(Stroke::new(1.0_f32, theme::ORANGE.gamma_multiply(0.7))),
                        )
                        .on_hover_text("Takes und Szenen zu einem Song arrangieren")
                        .clicked()
                    {
                        self.composer_open = true;
                    }
                    ui.separator();
                    ui.label(RichText::new("MIDI CLOCK").color(theme::MUTED).size(9.0));
                    let mut selected_mode = self.project.midi_clock_mode;
                    egui::ComboBox::from_id_salt("midi_clock_mode")
                        .selected_text(clock_mode_label(selected_mode))
                        .show_ui(ui, |ui| {
                            for mode in [
                                MidiClockMode::Internal,
                                MidiClockMode::Master,
                                MidiClockMode::Follower,
                            ] {
                                ui.selectable_value(
                                    &mut selected_mode,
                                    mode,
                                    clock_mode_label(mode),
                                );
                            }
                        });
                    if selected_mode != self.project.midi_clock_mode {
                        self.stop_transport();
                        self.project.midi_clock_mode = selected_mode;
                        self.clock_status = match selected_mode {
                            MidiClockMode::Internal => "CLOCK INTERN".to_owned(),
                            MidiClockMode::Master => "CLOCK MASTER BEREIT".to_owned(),
                            MidiClockMode::Follower => "WARTE AUF MIDI CLOCK".to_owned(),
                        };
                    }
                    ui.label(
                        RichText::new(&self.clock_status)
                            .color(
                                if self.project.midi_clock_mode == MidiClockMode::Follower
                                    && self
                                        .last_external_clock
                                        .is_none_or(|last| last.elapsed() > Duration::from_secs(1))
                                {
                                    theme::ORANGE
                                } else {
                                    theme::ACID
                                },
                            )
                            .size(9.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let file = self
                            .project_path
                            .as_ref()
                            .and_then(|path| path.file_name())
                            .and_then(|name| name.to_str())
                            .unwrap_or("AUTOSAVE-PROJEKT");
                        ui.label(RichText::new(file).color(theme::MUTED).size(9.0));
                    });
                });
                if ui.min_rect().bottom() > ui.clip_rect().bottom() {
                    ui.ctx().request_discard("toolbar height changed");
                }
            });
    }

    fn prepare_generator_preview(&mut self, mode: GenerationMode, advance_seed: bool) {
        self.project.ensure_generation_plan();
        if advance_seed {
            self.project.generation_plan.seed =
                self.project.generation_plan.seed.wrapping_add(1).max(1);
        }
        let lane = self.selected_lane;
        let kind = self.project.lanes[lane].kind;
        let current = self.project.lanes[lane].steps.clone();
        let steps = generate_lane(&self.project.generation_plan, lane, kind, &current, mode);
        self.generator_preview = Some(GeneratorPreview {
            lane,
            steps,
            seed: self.project.generation_plan.seed,
            mode,
        });
        self.generator_status = match mode {
            GenerationMode::Generate => format!(
                "VORSCHAU FÜR REIHE {} · SEED {}",
                lane + 1,
                self.project.generation_plan.seed
            ),
            GenerationMode::Mutate => format!(
                "MUTATION FÜR REIHE {} · SEED {}",
                lane + 1,
                self.project.generation_plan.seed
            ),
        };
    }

    fn apply_generator_preview(&mut self) {
        let Some(preview) = self.generator_preview.take() else {
            return;
        };
        if preview.lane != self.selected_lane {
            self.generator_status = "VORSCHAU GEHÖRT ZU EINER ANDEREN REIHE".to_owned();
            self.generator_preview = Some(preview);
            return;
        }
        self.project.lanes[preview.lane].steps = preview.steps;
        self.project.active_scene = None;
        if let Some(chain_index) = self.selected_chain_index() {
            let _ = self.project.sync_active_pattern_from_lanes(chain_index);
        }
        self.generator_status = format!("REIHE {} ÜBERNOMMEN ✓", preview.lane + 1);
    }

    fn generator_window(&mut self, ctx: &egui::Context) {
        if !self.generator_open {
            return;
        }
        self.project.ensure_generation_plan();
        let mut open = self.generator_open;
        let lane_index = self.selected_lane;
        let lane_kind = self.project.lanes[lane_index].kind;
        let lane_name = self.project.lanes[lane_index].name.clone();
        let mut config = self.project.generation_plan.lane_configs[lane_index].clone();
        let mut root = self.project.generation_plan.root;
        let mut scale = self.project.generation_plan.scale;
        let mut seed = self.project.generation_plan.seed;
        let preview = self.generator_preview.clone();
        let status = self.generator_status.clone();
        let mut action = None;

        egui::Window::new("✦ KOSMISCHER GENERATOR")
            .open(&mut open)
            .default_width(650.0)
            .resizable(true)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(10, 15, 18)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(format!("REIHE {} · {}", lane_index + 1, lane_name))
                            .color(theme::CYAN),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if lane_kind == LaneKind::Drums {
                                "DRUM-LABOR"
                            } else {
                                "BERLIN-SCHOOL-MOTIVLABOR"
                            })
                            .color(theme::MUTED)
                            .size(9.0),
                        );
                    });
                });
                ui.label(
                    RichText::new(
                        "Regelbasiert, reproduzierbar und ohne fremde Kompositionen zu klonen. Klaus besitzt schließlich Manieren.",
                    )
                    .color(theme::MUTED)
                    .italics(),
                );
                ui.separator();

                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("TONZENTRUM").color(theme::MUTED).size(9.0));
                    egui::ComboBox::from_id_salt("generator_root")
                        .selected_text(root_name(root))
                        .show_ui(ui, |ui| {
                            for candidate in 0..12 {
                                ui.selectable_value(&mut root, candidate, root_name(candidate));
                            }
                        });
                    if lane_kind == LaneKind::Melody {
                        egui::ComboBox::from_id_salt("generator_scale")
                            .selected_text(scale_label(scale))
                            .show_ui(ui, |ui| {
                                for candidate in all_scales() {
                                    ui.selectable_value(
                                        &mut scale,
                                        candidate,
                                        scale_label(candidate),
                                    );
                                }
                            });
                    }
                    ui.separator();
                    ui.label(RichText::new("SEED").color(theme::MUTED).size(9.0));
                    ui.add(egui::DragValue::new(&mut seed).speed(1.0));
                    ui.label(
                        RichText::new("gleicher Seed = gleiches Motiv")
                            .color(theme::MUTED)
                            .size(9.0),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new("ROLLE").color(theme::MUTED).size(9.0));
                    egui::ComboBox::from_id_salt("generator_role")
                        .selected_text(role_label(config.role))
                        .show_ui(ui, |ui| {
                            for role in roles_for_kind(lane_kind).iter().copied() {
                                ui.selectable_value(&mut config.role, role, role_label(role));
                            }
                        });
                    if lane_kind == LaneKind::Melody {
                        ui.separator();
                        ui.label(RichText::new("MIDI-BEREICH").color(theme::MUTED).size(9.0));
                        ui.add(
                            egui::DragValue::new(&mut config.min_pitch)
                                .range(0..=127)
                                .prefix("von "),
                        );
                        ui.add(
                            egui::DragValue::new(&mut config.max_pitch)
                                .range(0..=127)
                                .prefix("bis "),
                        );
                    }
                });

                ui.columns(2, |columns| {
                    columns[0].label(
                        RichText::new("STRUKTUR")
                            .color(theme::ORANGE)
                            .strong(),
                    );
                    columns[0].add(
                        egui::Slider::new(&mut config.motif_length, 1..=11)
                            .text("Motivlänge"),
                    );
                    columns[0].add(
                        egui::Slider::new(&mut config.effective_length, 1..=16)
                            .text("effektive Länge"),
                    );
                    columns[0].add(
                        egui::Slider::new(&mut config.density, 1..=100).text("Dichte %"),
                    );
                    columns[0]
                        .add(egui::Slider::new(&mut config.order, 0..=100).text("Ordnung %"));
                    columns[0].add(
                        egui::Slider::new(&mut config.movement, 0..=100).text("Bewegung %"),
                    );

                    columns[1].label(
                        RichText::new("LEBENDIGKEIT")
                            .color(theme::ORANGE)
                            .strong(),
                    );
                    columns[1].add(
                        egui::Slider::new(&mut config.heat, 0..=100)
                            .text("Hitze / Ratchets %"),
                    );
                    columns[1]
                        .add(egui::Slider::new(&mut config.drift, 0..=100).text("Drift %"));
                    columns[1].add(
                        egui::Slider::new(&mut config.mutation, 0..=100).text("Mutation %"),
                    );
                    columns[1].checkbox(&mut config.lock_notes, "Noten sperren");
                    columns[1].checkbox(&mut config.lock_rhythm, "Rhythmus sperren");
                    columns[1].checkbox(&mut config.lock_expression, "Ausdruck sperren");
                });

                if config.min_pitch > config.max_pitch {
                    std::mem::swap(&mut config.min_pitch, &mut config.max_pitch);
                }
                config.motif_length = config.motif_length.clamp(1, 11);
                config.effective_length = config.effective_length.clamp(1, 16);

                ui.separator();
                ui.label(RichText::new("VORSCHAU").color(theme::MUTED).size(9.0));
                ui.horizontal(|ui| {
                    let preview_matches = preview.as_ref().is_some_and(|value| value.lane == lane_index);
                    for step_index in 0..STEPS_PER_LANE {
                        let step = preview
                            .as_ref()
                            .filter(|value| value.lane == lane_index)
                            .map(|value| &value.steps[step_index]);
                        let (label, fill) = match step {
                            Some(step) if step.state == StepState::Skip => {
                                ("×".to_owned(), Color32::from_rgb(45, 27, 31))
                            }
                            Some(step) if step.state == StepState::Active => (
                                if step.ratchets > 1 {
                                    step.ratchets.to_string()
                                } else if step.notes.len() > 1 {
                                    format!("{}n", step.notes.len())
                                } else {
                                    "•".to_owned()
                                },
                                theme::lane_color(lane_index).gamma_multiply(0.72),
                            ),
                            _ => (String::new(), Color32::from_rgb(20, 27, 30)),
                        };
                        ui.add_sized(
                            [28.0, 32.0],
                            egui::Button::new(label)
                                .fill(fill)
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(56, 70, 75))),
                        );
                    }
                    if !preview_matches {
                        ui.label(RichText::new("noch kosmisch leer").color(theme::MUTED));
                    }
                });
                if let Some(value) = preview.as_ref().filter(|value| value.lane == lane_index) {
                    ui.label(
                        RichText::new(format!(
                            "{} · SEED {}",
                            generation_mode_label(value.mode),
                            value.seed
                        ))
                        .color(theme::CYAN)
                        .size(9.0),
                    );
                }

                ui.horizontal_wrapped(|ui| {
                    if ui.button("✦ ERZEUGEN").clicked() {
                        action = Some(GeneratorAction::Generate);
                    }
                    if ui.button("↻ VARIANTE").clicked() {
                        action = Some(GeneratorAction::Variant);
                    }
                    if ui.button("≈ MUTIEREN").clicked() {
                        action = Some(GeneratorAction::Mutate);
                    }
                    if ui
                        .add_enabled(
                            preview.as_ref().is_some_and(|value| value.lane == lane_index),
                            egui::Button::new("ÜBERNEHMEN"),
                        )
                        .clicked()
                    {
                        action = Some(GeneratorAction::Apply);
                    }
                    if ui
                        .add_enabled(preview.is_some(), egui::Button::new("VERWERFEN"))
                        .clicked()
                    {
                        action = Some(GeneratorAction::Discard);
                    }
                    if ui.button("ROLLENPROFIL ZURÜCKSETZEN").clicked() {
                        action = Some(GeneratorAction::ResetProfile);
                    }
                });
                ui.label(RichText::new(status).color(theme::ACID).size(9.0));
            });

        self.generator_open = open;
        self.project.generation_plan.root = root % 12;
        self.project.generation_plan.scale = scale;
        self.project.generation_plan.seed = seed.max(1);
        self.project.generation_plan.lane_configs[lane_index] = config;
        match action {
            Some(GeneratorAction::Generate) => {
                self.prepare_generator_preview(GenerationMode::Generate, false)
            }
            Some(GeneratorAction::Variant) => {
                self.prepare_generator_preview(GenerationMode::Generate, true)
            }
            Some(GeneratorAction::Mutate) => {
                self.prepare_generator_preview(GenerationMode::Mutate, true)
            }
            Some(GeneratorAction::Apply) => self.apply_generator_preview(),
            Some(GeneratorAction::Discard) => {
                self.generator_preview = None;
                self.generator_status = "VORSCHAU VERWORFEN".to_owned();
            }
            Some(GeneratorAction::ResetProfile) => {
                self.project.generation_plan.lane_configs[lane_index] =
                    LaneGeneratorConfig::for_lane(lane_index, lane_kind);
                self.generator_preview = None;
                self.generator_status = "ROLLENPROFIL ZURÜCKGESETZT".to_owned();
            }
            None => {}
        }
    }

    fn composer_window(&mut self, ctx: &egui::Context) {
        if !self.composer_open {
            return;
        }
        let mut open = self.composer_open;
        let takes: Vec<(usize, String)> = self
            .project
            .performance_takes
            .iter()
            .map(|take| (take.id, take.name.clone()))
            .collect();
        let scenes: Vec<(usize, String)> = self
            .project
            .scenes
            .iter()
            .map(|scene| (scene.id, scene.name.clone()))
            .collect();
        let clips = self.project.song_clips.clone();
        let mut add_source = None;
        let mut select_clip = self.selected_song_clip;
        let mut move_left = false;
        let mut move_right = false;
        let mut duplicate = false;
        let mut remove = false;
        let mut toggle_fullscreen = false;
        let mut play_song = false;
        let mut stop_song = false;
        let mut export_midi = false;

        egui::Window::new("▤ K.L.A.U.S. COMPOSER")
            .open(&mut open)
            .default_width(1120.0)
            .default_height(650.0)
            .resizable(true)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(8, 13, 17)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("SONG-ARRANGEMENT").color(theme::ORANGE));
                    ui.label(
                        RichText::new(format!(
                            "{} CLIPS · {} TAKTE",
                            clips.len(),
                            self.project.song_length_pulses().div_ceil(96)
                        ))
                        .color(theme::MUTED),
                    );
                    if ui
                        .add_enabled(!clips.is_empty(), egui::Button::new("▶ SONG"))
                        .clicked()
                    {
                        play_song = true;
                    }
                    if ui
                        .add_enabled(self.song_playback_active, egui::Button::new("■ STOPP"))
                        .clicked()
                    {
                        stop_song = true;
                    }
                    if ui
                        .add_enabled(!clips.is_empty(), egui::Button::new("⇩ MIDI EXPORT"))
                        .clicked()
                    {
                        export_midi = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(if self.composer_fullscreen {
                                "◱ FENSTER"
                            } else {
                                "□ VOLLBILD"
                            })
                            .clicked()
                        {
                            toggle_fullscreen = true;
                        }
                    });
                });
                ui.label(
                    RichText::new(
                        "Takes und Szenen werden nicht destruktiv als lineare Clips angeordnet.",
                    )
                    .color(theme::MUTED),
                );
                ui.separator();

                ui.columns(2, |columns| {
                    columns[0].set_width(245.0);
                    columns[0].heading(RichText::new("BIBLIOTHEK").color(theme::CYAN));
                    columns[0].label(RichText::new("PERFORMANCE-TAKES").color(theme::MUTED));
                    egui::ScrollArea::vertical()
                        .id_salt("composer_library")
                        .max_height(230.0)
                        .show(&mut columns[0], |ui| {
                            for (id, name) in &takes {
                                if ui
                                    .add_sized(
                                        [225.0, 34.0],
                                        egui::Button::new(format!("+ TAKE · {name}")),
                                    )
                                    .clicked()
                                {
                                    add_source = Some(SongClipSource::Take { take_id: *id });
                                }
                            }
                            ui.separator();
                            ui.label(RichText::new("SZENEN").color(theme::MUTED));
                            for (id, name) in &scenes {
                                if ui
                                    .add_sized(
                                        [225.0, 34.0],
                                        egui::Button::new(format!("+ SZENE · {name}")),
                                    )
                                    .clicked()
                                {
                                    add_source = Some(SongClipSource::Scene { scene_id: *id });
                                }
                            }
                        });

                    columns[1].heading(RichText::new("FAHRPLAN").color(theme::ORANGE));
                    egui::ScrollArea::horizontal()
                        .id_salt("composer_arrangement")
                        .show(&mut columns[1], |ui| {
                            ui.horizontal(|ui| {
                                for (index, clip) in clips.iter().enumerate() {
                                    let color = match clip.color_index % 4 {
                                        0 => theme::CYAN,
                                        1 => theme::ORANGE,
                                        2 => theme::VIOLET,
                                        _ => theme::ACID,
                                    };
                                    let width = (f32::from(clip.bars) * 20.0)
                                        .clamp(110.0, 300.0);
                                    let source = match clip.source {
                                        SongClipSource::Take { .. } => "TAKE",
                                        SongClipSource::Scene { .. } => "SZENE",
                                    };
                                    let response = ui.add_sized(
                                        [width, 88.0],
                                        egui::Button::new(
                                            RichText::new(format!(
                                                "{source}\n{}\n{} TAKTE × {}",
                                                clip.name, clip.bars, clip.repeats
                                            ))
                                            .color(Color32::WHITE),
                                        )
                                        .fill(color.gamma_multiply(0.28))
                                        .stroke(Stroke::new(
                                            if select_clip == Some(index) { 3.0_f32 } else { 1.0_f32 },
                                            color,
                                        )),
                                    );
                                    if response.clicked() {
                                        select_clip = Some(index);
                                    }
                                }
                                if clips.is_empty() {
                                    ui.add_sized(
                                        [460.0, 88.0],
                                        egui::Label::new(
                                            RichText::new(
                                                "TAKE ODER SZENE AUS DER BIBLIOTHEK EINFÜGEN",
                                            )
                                            .color(theme::MUTED),
                                        ),
                                    );
                                }
                            });
                        });

                    columns[1].separator();
                    if let Some(index) = select_clip
                        && let Some(clip) = self.project.song_clips.get_mut(index)
                    {
                        columns[1].heading(RichText::new("CLIP-INSPECTOR").color(theme::CYAN));
                        columns[1].horizontal_wrapped(|ui| {
                            ui.label("NAME");
                            ui.add(
                                egui::TextEdit::singleline(&mut clip.name).desired_width(180.0),
                            );
                            ui.label("TAKTE");
                            ui.add(egui::DragValue::new(&mut clip.bars).range(1..=256));
                            ui.label("WIEDERHOLUNGEN");
                            ui.add(egui::DragValue::new(&mut clip.repeats).range(1..=32));
                            ui.label("TRANSPOSE");
                            ui.add(egui::DragValue::new(&mut clip.transpose).range(-24..=24));
                        });
                        columns[1].horizontal_wrapped(|ui| {
                            ui.label("ÜBERGANG");
                            ui.selectable_value(
                                &mut clip.transition,
                                ClipTransition::Legato,
                                "LEGATO",
                            );
                            ui.selectable_value(
                                &mut clip.transition,
                                ClipTransition::Restart,
                                "NEUSTART",
                            );
                            if ui.button("←").clicked() {
                                move_left = true;
                            }
                            if ui.button("→").clicked() {
                                move_right = true;
                            }
                            if ui.button("⧉ DUPLIZIEREN").clicked() {
                                duplicate = true;
                            }
                            if ui.button("× LÖSCHEN").clicked() {
                                remove = true;
                            }
                        });
                        columns[1].label(
                            RichText::new(
                                "Transposition wirkt ausschließlich auf Melodie-Ketten; Drums bleiben standhaft.",
                            )
                            .color(theme::MUTED)
                            .size(9.0),
                        );
                    }
                });
            });

        self.composer_open = open;
        self.selected_song_clip = select_clip;
        if let Some(source) = add_source {
            self.selected_song_clip = Some(self.project.add_song_clip(source));
        }
        if let Some(index) = self.selected_song_clip {
            if move_left && index > 0 {
                self.project.song_clips.swap(index, index - 1);
                self.selected_song_clip = Some(index - 1);
            } else if move_right && index + 1 < self.project.song_clips.len() {
                self.project.song_clips.swap(index, index + 1);
                self.selected_song_clip = Some(index + 1);
            } else if duplicate && index < self.project.song_clips.len() {
                let mut copy = self.project.song_clips[index].clone();
                copy.id = self
                    .project
                    .song_clips
                    .iter()
                    .map(|clip| clip.id)
                    .max()
                    .map_or(0, |id| id + 1);
                copy.name = format!("{} KOPIE", copy.name);
                self.project.song_clips.insert(index + 1, copy);
                self.selected_song_clip = Some(index + 1);
            } else if remove && index < self.project.song_clips.len() {
                self.project.song_clips.remove(index);
                self.selected_song_clip = (!self.project.song_clips.is_empty())
                    .then_some(index.min(self.project.song_clips.len() - 1));
            }
        }
        if toggle_fullscreen {
            self.composer_fullscreen = !self.composer_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.composer_fullscreen));
        }
        if play_song {
            self.start_song_playback();
        }
        if stop_song {
            self.stop_transport();
        }
        if export_midi
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Standard MIDI File", &["mid"])
                .set_file_name(format!("{}.mid", self.project.title))
                .save_file()
        {
            self.save_status = match export_song(&self.project, &path) {
                Ok(()) => format!("MIDI EXPORTIERT · {}", path.display()),
                Err(error) => format!("MIDI-EXPORTFEHLER · {error}"),
            };
        }
    }

    fn performance_window(&mut self, ctx: &egui::Context) {
        if !self.performance_window_open {
            return;
        }
        let mut open = self.performance_window_open;
        let events = self.project.performance_timeline.clone();
        let status = self.performance_status.clone();
        let capture_active = self.performance_capture_active;
        let playback_active = self.performance_playback_active;
        let current_pulse = self.transport_pulses;
        let last_pulse = events.last().map_or(0, |event| event.pulse);
        let takes: Vec<(usize, String)> = self
            .project
            .performance_takes
            .iter()
            .enumerate()
            .map(|(index, take)| (index, take.name.clone()))
            .collect();
        let active_take = self.project.active_performance_take;
        let mut take_name = takes
            .get(active_take)
            .map_or_else(|| "TAKE".to_owned(), |(_, name)| name.clone());
        let active_loop = self.project.active_performance_loop().cloned();
        let mut loop_enabled = active_loop.is_some();
        let mut loop_start = active_loop.as_ref().map_or(0, |block| block.start_pulse);
        let mut loop_end = active_loop
            .as_ref()
            .map_or(last_pulse.max(96), |block| block.end_pulse);
        let mut loop_passes = active_loop.as_ref().map_or(2, |block| block.passes);
        let mut toggle_capture = false;
        let mut start_playback = false;
        let mut stop_playback = false;
        let mut clear_timeline = false;
        let mut remove_event = None;
        let mut select_take = None;
        let mut add_take = false;
        let mut duplicate_take = false;
        let mut remove_take = false;
        let mut move_event = None;
        let mut selected_event = self.selected_performance_event;

        egui::Window::new("● PERFORMANCE CAPTURE")
            .open(&mut open)
            .default_width(760.0)
            .default_height(560.0)
            .resizable(true)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(10, 15, 18)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(RichText::new("PERFORMANCE-TAKE").color(theme::CYAN));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("24 PPQN · {} EVENTS", events.len()))
                                .color(theme::MUTED)
                                .size(9.0),
                        );
                    });
                });
                ui.label(
                    RichText::new(
                        "Szenen, Pattern und Live-Skips werden auf der gemeinsamen Transportzeit aufgezeichnet.",
                    )
                    .color(theme::MUTED),
                );
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("TAKE").color(theme::CYAN).strong());
                    egui::ComboBox::from_id_salt("performance_take_select")
                        .selected_text(&take_name)
                        .show_ui(ui, |ui| {
                            for (index, name) in &takes {
                                if ui.selectable_label(*index == active_take, name).clicked() {
                                    select_take = Some(*index);
                                }
                            }
                        });
                    if ui
                        .add(egui::Button::new("+ NEU").min_size(egui::vec2(70.0, 30.0)))
                        .clicked()
                    {
                        add_take = true;
                    }
                    if ui
                        .add(
                            egui::Button::new("⧉ DUPLIZIEREN")
                                .min_size(egui::vec2(112.0, 30.0)),
                        )
                        .clicked()
                    {
                        duplicate_take = true;
                    }
                    if ui
                        .add_enabled(
                            takes.len() > 1,
                            egui::Button::new("− LÖSCHEN").min_size(egui::vec2(88.0, 30.0)),
                        )
                        .clicked()
                    {
                        remove_take = true;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut take_name)
                            .desired_width(170.0)
                            .hint_text("Take-Name"),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(if capture_active {
                                    "■ AUFNAHME BEENDEN"
                                } else {
                                    "● AUFNAHME BEREIT"
                                })
                                .color(Color32::WHITE),
                            )
                            .fill(if capture_active {
                                Color32::from_rgb(92, 28, 31)
                            } else {
                                Color32::from_rgb(54, 31, 32)
                            })
                            .min_size(egui::vec2(164.0, 38.0)),
                        )
                        .clicked()
                    {
                        toggle_capture = true;
                    }
                    if ui
                        .add_enabled(
                            !events.is_empty() && !capture_active,
                            egui::Button::new(
                                RichText::new("▶ TAKE ABSPIELEN").color(Color32::WHITE),
                            )
                                .fill(Color32::from_rgb(30, 61, 52))
                                .min_size(egui::vec2(142.0, 38.0)),
                        )
                        .clicked()
                    {
                        start_playback = true;
                    }
                    if ui
                        .add_enabled(
                            playback_active,
                            egui::Button::new(
                                RichText::new("■ TAKE STOPP").color(Color32::WHITE),
                            )
                                .min_size(egui::vec2(112.0, 38.0)),
                        )
                        .clicked()
                    {
                        stop_playback = true;
                    }
                    if ui
                        .add_enabled(
                            !events.is_empty() && !capture_active && !playback_active,
                            egui::Button::new("TIMELINE LEEREN"),
                        )
                        .on_hover_text("Den gespeicherten Performance-Take entfernen")
                        .clicked()
                    {
                        clear_timeline = true;
                    }
                });

                let progress = if last_pulse == 0 {
                    0.0
                } else {
                    (current_pulse as f32 / last_pulse as f32).clamp(0.0, 1.0)
                };
                let (progress_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 22.0),
                    Sense::hover(),
                );
                ui.painter()
                    .rect_filled(progress_rect, 6.0, Color32::from_rgb(22, 30, 34));
                if progress > 0.0 {
                    let mut filled = progress_rect;
                    filled.max.x = filled.min.x + filled.width() * progress;
                    ui.painter().rect_filled(
                        filled,
                        6.0,
                        if capture_active {
                            theme::DANGER
                        } else {
                            theme::CYAN
                        },
                    );
                }
                ui.painter().text(
                    progress_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!(
                        "{} · PULS {} / {}",
                        performance_position_label(current_pulse),
                        current_pulse,
                        last_pulse
                    ),
                    egui::FontId::proportional(10.0),
                    Color32::WHITE,
                );
                ui.label(RichText::new(status).color(theme::ACID).size(9.0));
                ui.separator();

                let timeline_max = last_pulse.max(loop_end).max(96);
                let (timeline_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 74.0),
                    Sense::hover(),
                );
                ui.painter().rect_filled(
                    timeline_rect,
                    5.0,
                    Color32::from_rgb(13, 21, 25),
                );
                let pulse_x = |pulse: u64| {
                    timeline_rect.left()
                        + timeline_rect.width() * pulse.min(timeline_max) as f32
                            / timeline_max as f32
                };
                for pulse in (0..=timeline_max).step_by(24) {
                    let x = pulse_x(pulse);
                    let is_bar = pulse % 96 == 0;
                    ui.painter().line_segment(
                        [egui::pos2(x, timeline_rect.top()), egui::pos2(x, timeline_rect.bottom())],
                        Stroke::new(
                            if is_bar { 1.5_f32 } else { 0.5_f32 },
                            if is_bar { theme::ORANGE } else { Color32::from_rgb(50, 65, 70) },
                        ),
                    );
                    if is_bar {
                        ui.painter().text(
                            egui::pos2(x + 4.0, timeline_rect.top() + 4.0),
                            egui::Align2::LEFT_TOP,
                            format!("{}", pulse / 96 + 1),
                            egui::FontId::monospace(9.0),
                            theme::MUTED,
                        );
                    }
                }
                if loop_enabled && loop_end > loop_start {
                    let loop_rect = egui::Rect::from_min_max(
                        egui::pos2(pulse_x(loop_start), timeline_rect.top() + 18.0),
                        egui::pos2(pulse_x(loop_end), timeline_rect.bottom()),
                    );
                    ui.painter().rect_filled(
                        loop_rect,
                        3.0,
                        Color32::from_rgba_unmultiplied(157, 105, 227, 38),
                    );
                }
                for (index, event) in events.iter().enumerate() {
                    let x = pulse_x(event.pulse);
                    let y = timeline_rect.center().y + (index % 3) as f32 * 11.0 - 11.0;
                    let marker = egui::Rect::from_center_size(
                        egui::pos2(x, y),
                        egui::vec2(30.0, 40.0),
                    );
                    let response = ui.interact(
                        marker,
                        ui.id().with(("performance_event", index)),
                        Sense::click_and_drag(),
                    );
                    if !capture_active && !playback_active && response.clicked() {
                        selected_event = Some(index);
                    }
                    if !capture_active && !playback_active && response.dragged() {
                        let relative = ((response.interact_pointer_pos().map_or(x, |pos| pos.x)
                            - timeline_rect.left())
                            / timeline_rect.width())
                            .clamp(0.0, 1.0);
                        move_event = Some((index, (relative * timeline_max as f32).round() as u64));
                        selected_event = Some(index);
                    }
                    ui.painter().circle_filled(
                        egui::pos2(x, y),
                        if selected_event == Some(index) { 7.0 } else { 5.0 },
                        if selected_event == Some(index) { theme::ACID } else { theme::CYAN },
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(
                        &mut loop_enabled,
                        RichText::new("WIEDERHOLUNGSBLOCK").color(Color32::WHITE),
                    );
                    ui.label(RichText::new("START").color(theme::MUTED));
                    ui.add_sized(
                        [62.0, 30.0],
                        egui::DragValue::new(&mut loop_start).range(0..=999_999),
                    );
                    ui.label(RichText::new("ENDE").color(theme::MUTED));
                    ui.add_sized(
                        [62.0, 30.0],
                        egui::DragValue::new(&mut loop_end).range(1..=999_999),
                    );
                    ui.label(RichText::new("DURCHLÄUFE").color(theme::MUTED));
                    ui.add_sized(
                        [52.0, 30.0],
                        egui::DragValue::new(&mut loop_passes).range(2..=16),
                    );
                });
                ui.label(
                    RichText::new("MARKER ANTIPPEN UND ZIEHEN · RASTER: 24 PPQN")
                        .color(theme::MUTED)
                        .size(8.0),
                );
                ui.separator();

                if events.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("NOCH KEIN TAKE")
                                .color(theme::MUTED)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(
                                "Aufnahme bereit schalten, Transport starten und performen.",
                            )
                            .color(theme::MUTED)
                            .italics(),
                        );
                    });
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(290.0)
                        .show(ui, |ui| {
                            for (index, event) in events.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    ui.add_sized(
                                        [100.0, 24.0],
                                        egui::Label::new(
                                            RichText::new(performance_position_label(event.pulse))
                                                .color(theme::ORANGE)
                                                .monospace(),
                                        ),
                                    );
                                    ui.add_sized(
                                        [72.0, 24.0],
                                        egui::Label::new(
                                            RichText::new(format!("P {:05}", event.pulse))
                                                .color(theme::MUTED)
                                                .monospace(),
                                        ),
                                    );
                                    ui.label(
                                        RichText::new(performance_action_label(
                                            &event.action,
                                            &self.project,
                                        ))
                                        .color(Color32::WHITE),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .add_enabled(
                                                !capture_active && !playback_active,
                                                egui::Button::new("×")
                                                    .min_size(egui::vec2(32.0, 30.0)),
                                            )
                                            .on_hover_text("Ereignis entfernen")
                                            .clicked()
                                        {
                                            remove_event = Some(index);
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                }
            });

        self.performance_window_open = open;
        self.selected_performance_event = selected_event;
        let take_management_action =
            select_take.is_some() || add_take || duplicate_take || remove_take;
        if !capture_active && !playback_active {
            if let Some(index) = select_take {
                if self.project.select_performance_take(index) {
                    self.transport_pulses = 0;
                    self.performance_playback_cursor = 0;
                    self.selected_performance_event = None;
                    self.performance_status =
                        format!("{} GELADEN", self.project.performance_takes[index].name);
                }
            } else if add_take {
                self.project.add_performance_take(false);
                self.transport_pulses = 0;
                self.selected_performance_event = None;
                self.performance_status = "NEUER TAKE BEREIT".to_owned();
            } else if duplicate_take {
                self.project.add_performance_take(true);
                self.transport_pulses = 0;
                self.selected_performance_event = None;
                self.performance_status = "TAKE DUPLIZIERT".to_owned();
            } else if remove_take && self.project.remove_active_performance_take() {
                self.transport_pulses = 0;
                self.selected_performance_event = None;
                self.performance_status = "TAKE GELÖSCHT".to_owned();
            }
        }
        if !take_management_action
            && !capture_active
            && !playback_active
            && let Some(take) = self
                .project
                .performance_takes
                .get_mut(self.project.active_performance_take)
        {
            let trimmed = take_name.trim();
            if !trimmed.is_empty() {
                take.name = trimmed.chars().take(40).collect();
            }
            take.loop_block = loop_enabled.then(|| PerformanceLoop {
                start_pulse: loop_start.min(loop_end.saturating_sub(1)),
                end_pulse: loop_end.max(loop_start + 1),
                passes: loop_passes.clamp(2, 16),
            });
        }
        if !take_management_action
            && let Some((index, pulse)) = move_event
            && let Some(event) = self.project.performance_timeline.get_mut(index)
        {
            event.pulse = pulse;
            self.project
                .performance_timeline
                .sort_by_key(|event| event.pulse);
            self.project.sync_active_performance_take();
            self.performance_status = format!("EREIGNIS VERSCHOBEN · PULS {pulse}");
        }
        if toggle_capture {
            if self.performance_capture_active {
                self.performance_capture_active = false;
                self.performance_status = format!(
                    "AUFNAHME BEENDET · {} EVENTS",
                    self.project.performance_timeline.len()
                );
            } else {
                if self.performance_playback_active {
                    self.stop_performance_playback();
                }
                if !self.engine.playing
                    && let Some(last_event) = self.project.performance_timeline.last()
                {
                    let continuation_pulse = last_event.pulse;
                    if matches!(&last_event.action, PerformanceAction::TransportStop) {
                        self.project.performance_timeline.pop();
                        self.project.sync_active_performance_take();
                    }
                    self.transport_pulses = continuation_pulse;
                    self.clock_accumulator = 0.0;
                }
                self.performance_capture_active = true;
                self.mode = WorkMode::Perform;
                self.performance_status = format!(
                    "AUFNAHME BEREIT · TRANSPORT STARTEN · PULS {}",
                    self.transport_pulses
                );
            }
        }
        if start_playback {
            self.start_performance_playback();
        }
        if stop_playback {
            self.stop_performance_playback();
        }
        if clear_timeline {
            self.project.performance_timeline.clear();
            self.project.sync_active_performance_take();
            self.performance_playback_cursor = 0;
            self.performance_status = "TIMELINE GELEERT".to_owned();
        }
        if let Some(index) = remove_event
            && index < self.project.performance_timeline.len()
        {
            self.project.performance_timeline.remove(index);
            self.project.sync_active_performance_take();
            self.performance_playback_cursor = 0;
            self.performance_status = "EREIGNIS ENTFERNT".to_owned();
        }
    }

    fn scenes_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("scenes_bar")
            .min_height(64.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(13, 19, 22))
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("SCENES").color(theme::CYAN).strong());
                    egui::ComboBox::from_id_salt("scene_quantization")
                        .selected_text(scene_quantization_label(
                            self.project.scene_launch_quantization,
                        ))
                        .show_ui(ui, |ui| {
                            for quantization in [
                                SceneLaunchQuantization::Immediate,
                                SceneLaunchQuantization::NextStep,
                                SceneLaunchQuantization::NextBeat,
                                SceneLaunchQuantization::NextBar,
                            ] {
                                ui.selectable_value(
                                    &mut self.project.scene_launch_quantization,
                                    quantization,
                                    scene_quantization_label(quantization),
                                );
                            }
                        });
                    if ui.button("+ SCENE").clicked() {
                        let name = format!("SCENE {:02}", self.project.scenes.len() + 1);
                        let index = self.project.add_scene(name.clone());
                        self.scene_status = format!("{name} ANGELEGT");
                        self.project.active_scene = Some(index);
                    }
                    if ui
                        .add_enabled(
                            self.project.active_scene.is_some(),
                            egui::Button::new("AKTUALISIEREN"),
                        )
                        .on_hover_text("Aktuelle Kettenpatterns in der aktiven Szene speichern")
                        .clicked()
                        && let Some(index) = self.project.active_scene
                    {
                        if let Err(error) = self.project.capture_scene(index) {
                            self.scene_status = format!("SZENENFEHLER · {error}");
                        } else {
                            self.scene_status = "SZENE AKTUALISIERT ✓".to_owned();
                        }
                    }
                    if let Some(index) = self.project.active_scene
                        && let Some(scene) = self.project.scenes.get_mut(index)
                    {
                        ui.add_sized(
                            [132.0, 36.0],
                            egui::TextEdit::singleline(&mut scene.name).hint_text("Szenenname"),
                        );
                    }
                    ui.separator();
                    let scenes: Vec<(usize, String)> = self
                        .project
                        .scenes
                        .iter()
                        .enumerate()
                        .map(|(index, scene)| (index, scene.name.clone()))
                        .collect();
                    egui::ScrollArea::horizontal()
                        .id_salt("scene_buttons")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, name) in scenes {
                                    let active = self.project.active_scene == Some(index);
                                    let queued = self.queued_scene == Some(index);
                                    let label = if queued {
                                        format!("{name} · NEXT")
                                    } else if active {
                                        format!("{name} · AKTIV")
                                    } else {
                                        name
                                    };
                                    if ui
                                        .add(egui::Button::new(label).fill(if active {
                                            Color32::from_rgb(35, 95, 89)
                                        } else if queued {
                                            Color32::from_rgb(79, 54, 104)
                                        } else {
                                            theme::PANEL_RAISED
                                        }))
                                        .clicked()
                                    {
                                        self.request_scene(index);
                                    }
                                }
                            });
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(&self.scene_status)
                                .color(theme::MUTED)
                                .size(9.0),
                        );
                    });
                });
                if ui.min_rect().bottom() > ui.clip_rect().bottom() {
                    ui.ctx().request_discard("toolbar height changed");
                }
            });
    }

    fn midi_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("midi_bar")
            .min_height(76.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(12, 18, 21))
                    .inner_margin(egui::Margin::symmetric(12, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
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
                ui.horizontal_wrapped(|ui| {
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
                if ui.min_rect().bottom() > ui.clip_rect().bottom() {
                    ui.ctx().request_discard("toolbar height changed");
                }
            });
    }

    fn autosave_if_needed(&mut self) {
        if self.last_autosave.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_autosave = Instant::now();
        self.project.sync_all_active_patterns();
        self.project.sync_active_performance_take();
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

    fn capture_history_change(&mut self) {
        self.project.sync_all_active_patterns();
        self.project.sync_active_performance_take();
        if self.project.midi_clock_mode == MidiClockMode::Follower {
            self.history_project.bpm = self.project.bpm;
        }
        if self.project == self.history_project {
            return;
        }
        self.undo_stack.push(self.history_project.clone());
        if self.undo_stack.len() > 64 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
        self.history_project = self.project.clone();
    }

    fn undo(&mut self) {
        self.capture_history_change();
        let Some(previous) = self.undo_stack.pop() else {
            self.save_status = "NICHTS RÜCKGÄNGIG ZU MACHEN".to_owned();
            return;
        };
        self.redo_stack.push(self.project.clone());
        self.project = previous;
        self.history_project = self.project.clone();
        self.queued_scene = None;
        self.save_status = "RÜCKGÄNGIG ✓".to_owned();
    }

    fn redo(&mut self) {
        let Some(next) = self.redo_stack.pop() else {
            self.save_status = "NICHTS ZU WIEDERHOLEN".to_owned();
            return;
        };
        self.undo_stack.push(self.project.clone());
        self.project = next;
        self.history_project = self.project.clone();
        self.queued_scene = None;
        self.save_status = "WIEDERHOLEN ✓".to_owned();
    }

    fn save_project_to(&mut self, path: PathBuf) {
        self.project.sync_all_active_patterns();
        self.project.sync_active_performance_take();
        match save_project_atomic(&self.project, &path) {
            Ok(()) => {
                self.project_path = Some(path);
                self.last_saved_json = self.project.to_json().unwrap_or_default();
                self.save_status = "PROJEKT GESPEICHERT ✓".to_owned();
            }
            Err(error) => self.save_status = format!("SPEICHERFEHLER · {error}"),
        }
    }

    fn save_project(&mut self) {
        if let Some(path) = self.project_path.clone() {
            self.save_project_to(path);
        } else {
            self.save_project_as();
        }
    }

    fn save_project_as(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("K.L.A.U.S.-Projekt speichern")
            .add_filter("K.L.A.U.S.-Projekt", &["json"])
            .set_file_name("night-drive.klaus58.json")
            .save_file()
        {
            self.save_project_to(path);
        }
    }

    fn open_project(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("K.L.A.U.S.-Projekt öffnen")
            .add_filter("K.L.A.U.S.-Projekt", &["json"])
            .pick_file()
        else {
            return;
        };
        match load_project(&path) {
            Ok(project) => {
                self.engine.stop();
                self.midi.panic();
                self.project = project;
                self.project_path = Some(path);
                self.selected_lane = self.selected_lane.min(self.project.lanes.len() - 1);
                self.selected_step = self.selected_step.min(STEPS_PER_LANE - 1);
                self.queued_scene = None;
                self.performance_capture_active = false;
                self.performance_playback_active = false;
                self.performance_playback_cursor = 0;
                self.transport_pulses = 0;
                self.clock_accumulator = 0.0;
                self.undo_stack.clear();
                self.redo_stack.clear();
                self.history_project = self.project.clone();
                self.last_saved_json = self.project.to_json().unwrap_or_default();
                self.save_status = "PROJEKT GEÖFFNET ✓".to_owned();
            }
            Err(error) => self.save_status = format!("ÖFFNUNGSFEHLER · {error}"),
        }
    }

    fn record_performance_action(&mut self, action: PerformanceAction) {
        if !self.performance_capture_active || self.performance_playback_active {
            return;
        }
        let event = PerformanceEvent {
            pulse: self.transport_pulses,
            action,
        };
        if self
            .project
            .performance_timeline
            .last()
            .is_some_and(|last| *last == event)
        {
            return;
        }
        self.project.performance_timeline.push(event);
        self.project
            .performance_timeline
            .sort_by_key(|event| event.pulse);
        self.project.sync_active_performance_take();
        self.performance_status = format!(
            "AUFNAHME · {} EVENTS · PULS {}",
            self.project.performance_timeline.len(),
            self.transport_pulses
        );
    }

    fn set_live_skip(&mut self, lane: usize, step: usize, enabled: bool) {
        let changed = if enabled {
            self.live_skips.insert((lane, step))
        } else {
            self.live_skips.remove(&(lane, step))
        };
        if changed {
            self.record_performance_action(PerformanceAction::LiveSkip {
                lane,
                step,
                enabled,
            });
        }
    }

    fn clear_live_skips_recorded(&mut self) {
        let skips: Vec<(usize, usize)> = self.live_skips.iter().copied().collect();
        for (lane, step) in skips {
            self.set_live_skip(lane, step, false);
        }
    }

    fn start_performance_playback(&mut self) {
        if self.project.performance_timeline.is_empty() {
            self.performance_status = "KEIN TAKE ZUM ABSPIELEN".to_owned();
            return;
        }
        self.performance_capture_active = false;
        self.performance_playback_active = false;
        self.engine.stop();
        self.midi.panic();
        self.live_skips.clear();
        self.queued_scene = None;
        self.transport_pulses = 0;
        self.clock_accumulator = 0.0;
        self.external_clock_pulses = 0;
        self.performance_playback_cursor = 0;
        self.performance_loop_pass = 1;
        self.performance_playback_active = true;
        self.apply_performance_events_through(0);
        if self.performance_playback_active {
            self.toggle_transport();
            self.performance_status = "TAKE LÄUFT · PULS 0".to_owned();
        }
    }

    fn apply_song_action(&mut self, action: PerformanceAction) {
        match action {
            PerformanceAction::SceneLaunch { scene_id } => {
                if let Some(index) = self
                    .project
                    .scenes
                    .iter()
                    .position(|scene| scene.id == scene_id)
                {
                    self.apply_scene_now(index);
                }
            }
            PerformanceAction::PatternChange {
                chain_id,
                pattern_id,
            } => {
                let chain = self
                    .project
                    .chains
                    .iter()
                    .position(|item| item.id == chain_id);
                let pattern = self
                    .project
                    .patterns
                    .iter()
                    .position(|item| item.id == pattern_id);
                if let (Some(chain), Some(pattern)) = (chain, pattern) {
                    let _ = self.project.apply_chain_pattern(chain, pattern);
                }
            }
            PerformanceAction::LiveSkip {
                lane,
                step,
                enabled,
            } => {
                if enabled {
                    self.live_skips.insert((lane, step));
                } else {
                    self.live_skips.remove(&(lane, step));
                }
            }
            PerformanceAction::TransportStart | PerformanceAction::TransportStop => {}
        }
    }

    fn apply_song_clip(&mut self, index: usize) {
        let Some(clip) = self.project.song_clips.get(index).cloned() else {
            return;
        };
        self.midi.panic();
        if clip.transition == ClipTransition::Restart {
            self.engine.stop();
            self.engine.start();
        }
        self.song_transpose = clip.transpose;
        self.live_skips.clear();
        match clip.source {
            SongClipSource::Scene { scene_id } => {
                self.apply_song_action(PerformanceAction::SceneLaunch { scene_id });
            }
            SongClipSource::Take { take_id } => {
                let actions: Vec<_> = self
                    .project
                    .performance_takes
                    .iter()
                    .find(|take| take.id == take_id)
                    .map(|take| {
                        take.events
                            .iter()
                            .filter(|event| event.pulse == 0)
                            .map(|event| event.action.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for action in actions {
                    self.apply_song_action(action);
                }
            }
        }
    }

    fn start_song_playback(&mut self) {
        if self.project.song_clips.is_empty() {
            return;
        }
        self.stop_transport();
        self.transport_pulses = 0;
        self.song_playback_active = true;
        self.song_clip_index = 0;
        self.apply_song_clip(0);
        self.engine.start();
    }

    fn update_song_playback(&mut self, previous: u64, current: u64) {
        if !self.song_playback_active {
            return;
        }
        let total = self.project.song_length_pulses();
        if current >= total {
            self.song_playback_active = false;
            self.stop_transport();
            return;
        }
        let mut start = 0;
        let mut found = 0;
        for (index, clip) in self.project.song_clips.iter().enumerate() {
            if current < start + clip.total_pulses() {
                found = index;
                break;
            }
            start += clip.total_pulses();
        }
        if found != self.song_clip_index {
            self.song_clip_index = found;
            self.apply_song_clip(found);
        }
        let Some(clip) = self.project.song_clips.get(found).cloned() else {
            return;
        };
        if let SongClipSource::Take { take_id } = clip.source {
            let events = self
                .project
                .performance_takes
                .iter()
                .find(|take| take.id == take_id)
                .map(|take| take.events.clone())
                .unwrap_or_default();
            let cycle = events.last().map_or(96, |event| event.pulse.max(96));
            for pulse in previous.saturating_add(1)..=current {
                let local = (pulse.saturating_sub(start)) % cycle;
                for action in events
                    .iter()
                    .filter(|event| event.pulse == local)
                    .map(|event| event.action.clone())
                {
                    self.apply_song_action(action);
                }
            }
        }
    }

    fn stop_performance_playback(&mut self) {
        self.performance_playback_active = false;
        self.performance_playback_cursor = 0;
        self.performance_loop_pass = 1;
        self.engine.stop();
        self.midi.panic();
        self.transport_pulses = 0;
        self.clock_accumulator = 0.0;
        self.external_clock_pulses = 0;
        self.performance_status = "TAKE GESTOPPT".to_owned();
    }

    fn apply_performance_events_through(&mut self, current_pulse: u64) {
        if !self.performance_playback_active {
            return;
        }
        let loop_block = self.project.active_performance_loop().cloned();
        let can_repeat = loop_block
            .as_ref()
            .is_some_and(|block| self.performance_loop_pass < block.passes);
        let mut stop_at_end = false;
        while let Some(event) = self
            .project
            .performance_timeline
            .get(self.performance_playback_cursor)
            .cloned()
        {
            if event.pulse > current_pulse {
                break;
            }
            self.performance_playback_cursor += 1;
            match event.action {
                PerformanceAction::SceneLaunch { scene_id } => {
                    if let Some(scene_index) = self
                        .project
                        .scenes
                        .iter()
                        .position(|scene| scene.id == scene_id)
                    {
                        self.apply_scene_now(scene_index);
                    }
                }
                PerformanceAction::PatternChange {
                    chain_id,
                    pattern_id,
                } => {
                    let chain_index = self
                        .project
                        .chains
                        .iter()
                        .position(|chain| chain.id == chain_id);
                    let pattern_index = self
                        .project
                        .patterns
                        .iter()
                        .position(|pattern| pattern.id == pattern_id);
                    if let (Some(chain_index), Some(pattern_index)) = (chain_index, pattern_index) {
                        let _ = self.project.apply_chain_pattern(chain_index, pattern_index);
                    }
                }
                PerformanceAction::LiveSkip {
                    lane,
                    step,
                    enabled,
                } => {
                    if enabled {
                        self.live_skips.insert((lane, step));
                    } else {
                        self.live_skips.remove(&(lane, step));
                    }
                }
                PerformanceAction::TransportStart => {}
                PerformanceAction::TransportStop => {
                    stop_at_end = !can_repeat
                        || loop_block
                            .as_ref()
                            .is_none_or(|block| event.pulse < block.end_pulse);
                }
            }
        }
        if let Some(block) = loop_block
            && current_pulse >= block.end_pulse
            && self.performance_loop_pass < block.passes
        {
            self.performance_loop_pass += 1;
            self.engine.stop();
            self.midi.panic();
            self.engine.start();
            self.live_skips.clear();
            self.transport_pulses = block.start_pulse;
            self.performance_playback_cursor = 0;
            self.apply_performance_events_through(block.start_pulse);
            self.performance_status = format!(
                "TAKE-SCHLEIFE · DURCHLAUF {} / {}",
                self.performance_loop_pass, block.passes
            );
            return;
        }
        if stop_at_end {
            self.performance_playback_active = false;
            self.engine.stop();
            self.midi.panic();
            self.performance_status = format!("TAKE BEENDET · PULS {current_pulse}");
        } else {
            self.performance_status = format!("TAKE LÄUFT · PULS {current_pulse}");
        }
    }

    fn request_scene(&mut self, scene_index: usize) {
        if !self.engine.playing
            || self.project.scene_launch_quantization == SceneLaunchQuantization::Immediate
        {
            self.apply_scene_now(scene_index);
        } else {
            self.queued_scene = Some(scene_index);
            let name = self
                .project
                .scenes
                .get(scene_index)
                .map_or("SZENE", |scene| scene.name.as_str());
            self.scene_status = format!("{name} VORGEMERKT");
        }
    }

    fn apply_scene_now(&mut self, scene_index: usize) {
        match self.project.apply_scene(scene_index) {
            Ok(()) => {
                self.queued_scene = None;
                if let Some(scene_id) = self.project.scenes.get(scene_index).map(|scene| scene.id) {
                    self.record_performance_action(PerformanceAction::SceneLaunch { scene_id });
                }
                let name = self
                    .project
                    .scenes
                    .get(scene_index)
                    .map_or("SZENE", |scene| scene.name.as_str());
                self.scene_status = format!("{name} AKTIV");
            }
            Err(error) => self.scene_status = format!("SZENENFEHLER · {error}"),
        }
    }

    fn maybe_launch_queued_scene(&mut self, previous_pulse: u64, current_pulse: u64) {
        let Some(scene_index) = self.queued_scene else {
            return;
        };
        let Some(boundary) = self.project.scene_launch_quantization.pulses() else {
            self.apply_scene_now(scene_index);
            return;
        };
        if quantization_boundary_crossed(previous_pulse, current_pulse, boundary) {
            self.apply_scene_now(scene_index);
        }
    }

    fn advance_transport_clock(&mut self, delta_seconds: f64) -> f64 {
        if !self.engine.playing {
            self.external_clock_pulses = 0;
            return 0.0;
        }
        match self.project.midi_clock_mode {
            MidiClockMode::Follower => {
                let pulses = std::mem::take(&mut self.external_clock_pulses);
                let previous = self.transport_pulses;
                self.transport_pulses = self.transport_pulses.wrapping_add(pulses);
                self.maybe_launch_queued_scene(previous, self.transport_pulses);
                self.apply_performance_events_through(self.transport_pulses);
                self.update_song_playback(previous, self.transport_pulses);
                f64::from(pulses as u32) * 60.0 / (f64::from(self.project.bpm) * 24.0)
            }
            MidiClockMode::Internal | MidiClockMode::Master => {
                self.clock_accumulator +=
                    delta_seconds.max(0.0) * f64::from(self.project.bpm) / 60.0 * 24.0;
                let pulses = self.clock_accumulator.floor() as u64;
                self.clock_accumulator -= pulses as f64;
                let previous = self.transport_pulses;
                self.transport_pulses = self.transport_pulses.wrapping_add(pulses);
                if self.project.midi_clock_mode == MidiClockMode::Master {
                    let now = Instant::now();
                    for _ in 0..pulses {
                        self.midi.schedule_realtime(now, 0xF8);
                    }
                }
                self.maybe_launch_queued_scene(previous, self.transport_pulses);
                self.apply_performance_events_through(self.transport_pulses);
                self.update_song_playback(previous, self.transport_pulses);
                delta_seconds
            }
        }
    }

    fn toggle_transport(&mut self) {
        if self.engine.playing {
            self.engine.pause();
            if self.project.midi_clock_mode == MidiClockMode::Master {
                self.midi.schedule_realtime(Instant::now(), 0xFC);
            }
        } else {
            if self.project.midi_clock_mode == MidiClockMode::Master {
                self.midi.schedule_realtime(
                    Instant::now(),
                    if self.transport_pulses == 0 {
                        0xFA
                    } else {
                        0xFB
                    },
                );
            }
            self.engine.start();
            self.record_performance_action(PerformanceAction::TransportStart);
        }
    }

    fn stop_transport(&mut self) {
        if self.engine.playing || self.performance_capture_active {
            self.record_performance_action(PerformanceAction::TransportStop);
        }
        self.performance_capture_active = false;
        self.performance_playback_active = false;
        self.song_playback_active = false;
        self.performance_playback_cursor = 0;
        if self.project.midi_clock_mode == MidiClockMode::Master {
            self.midi.schedule_realtime(Instant::now(), 0xFC);
        }
        self.engine.stop();
        self.midi.panic();
        self.transport_pulses = 0;
        self.clock_accumulator = 0.0;
        self.external_clock_pulses = 0;
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
                MidiEvent::Status(status) => {
                    let became_connected = !matches!(self.midi_status, MidiStatus::Connected(_))
                        && matches!(status, MidiStatus::Connected(_));
                    self.midi_status = status;
                    if became_connected {
                        self.send_automatic_sounds();
                    }
                }
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
        if self.project.midi_clock_mode == MidiClockMode::Follower && message.len >= 1 {
            match message.data[0] {
                0xF8 => {
                    let now = Instant::now();
                    if let Some(previous) = self.last_external_clock {
                        let elapsed = now.duration_since(previous).as_secs_f64();
                        if elapsed > 0.001 {
                            let measured = (60.0 / (elapsed * 24.0)).clamp(20.0, 300.0) as f32;
                            self.project.bpm = self.project.bpm * 0.8 + measured * 0.2;
                        }
                    }
                    self.last_external_clock = Some(now);
                    self.clock_status = format!("CLOCK FOLLOWER · {:.1} BPM", self.project.bpm);
                    if self.engine.playing {
                        self.external_clock_pulses = self.external_clock_pulses.wrapping_add(1);
                    }
                    return;
                }
                0xFA => {
                    self.engine.stop();
                    self.transport_pulses = 0;
                    self.clock_accumulator = 0.0;
                    self.external_clock_pulses = 0;
                    self.engine.start();
                    self.clock_status = "CLOCK START EMPFANGEN".to_owned();
                    return;
                }
                0xFB => {
                    self.engine.start();
                    self.clock_status = "CLOCK CONTINUE EMPFANGEN".to_owned();
                    return;
                }
                0xFC => {
                    self.engine.pause();
                    self.midi.panic();
                    self.clock_status = "CLOCK STOP EMPFANGEN".to_owned();
                    return;
                }
                _ => {}
            }
        }

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
            Ok(_) => {
                self.project.active_scene = None;
                self.arp_status = format!("ARP TAKE {take_number:02} AKTIV");
            }
            Err(error) => self.arp_status = format!("RENDER-FEHLER · {error}"),
        }
    }

    fn send_chain_sound(&mut self, chain_index: usize) {
        if !matches!(self.midi_status, MidiStatus::Connected(_)) {
            self.sound_status = "SOUND NICHT GESENDET · MIDI OUT GETRENNT".to_owned();
            return;
        }
        let Some(chain) = self.project.chains.get(chain_index) else {
            return;
        };
        let sound = chain.route.sound.clone();
        if !sound.enabled {
            self.sound_status = format!("{} · SOUND-ZIEL IST AUS", chain.name);
            return;
        }
        let now = Instant::now();
        let channel = chain.route.channel.clamp(1, 16);
        for (message, len) in
            sound_selection_messages(channel, sound.bank_msb, sound.bank_lsb, sound.program)
        {
            self.midi.schedule_short(now, message, len);
        }
        self.sound_status = format!(
            "{} · BANK {}:{} · PROGRAMM {} GESENDET ✓",
            chain.name,
            sound
                .bank_msb
                .map_or_else(|| "–".to_owned(), |value| value.to_string()),
            sound
                .bank_lsb
                .map_or_else(|| "–".to_owned(), |value| value.to_string()),
            u16::from(sound.program) + 1
        );
    }

    fn send_automatic_sounds(&mut self) {
        let chain_indices: Vec<usize> = self
            .project
            .chains
            .iter()
            .enumerate()
            .filter_map(|(index, chain)| {
                (chain.route.sound.enabled && chain.route.sound.send_on_connect).then_some(index)
            })
            .collect();
        for chain_index in chain_indices {
            self.send_chain_sound(chain_index);
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
                    let pitch = if self.song_playback_active && lane.kind == LaneKind::Melody {
                        (i16::from(note.pitch) + i16::from(self.song_transpose)).clamp(0, 127) as u8
                    } else {
                        note.pitch
                    };
                    let gate_seconds = (slice_seconds * f64::from(note.gate.clamp(0.05, 1.0)))
                        .clamp(0.001, slice_seconds * 0.98);
                    self.midi
                        .schedule(note_time, note_on(channel, pitch, note.velocity));
                    self.midi.schedule(
                        note_time + Duration::from_secs_f64(gate_seconds),
                        note_off(channel, pitch),
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
                        let mut send_sound = None;
                        let mut link_with_next = false;
                        let mut split_chain = false;
                        let midi_connected = matches!(self.midi_status, MidiStatus::Connected(_));
                        let sound_status = self.sound_status.clone();
                        if let Some(chain_index) = self
                            .project
                            .chains
                            .iter()
                            .position(|chain| chain.lane_ids.contains(&self.selected_lane))
                        {
                            let can_link = self
                                .project
                                .can_link_lane_with_next(self.selected_lane);
                            let can_split = self
                                .project
                                .can_split_chain_for_lane(self.selected_lane);
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
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        can_link,
                                        egui::Button::new(RichText::new(
                                            "MIT NÄCHSTER REIHE VERKETTEN",
                                        ))
                                        .fill(Color32::from_rgb(24, 72, 74))
                                        .min_size(egui::vec2(ui.available_width(), 34.0)),
                                    )
                                    .on_hover_text(
                                        "Nur benachbarte Reihen desselben Typs können eine Kette bilden",
                                    )
                                    .clicked()
                                {
                                    link_with_next = true;
                                }
                            });
                            if ui
                                .add_enabled(
                                    can_split,
                                    egui::Button::new(RichText::new("KETTE TRENNEN"))
                                        .fill(Color32::from_rgb(82, 47, 28))
                                        .min_size(egui::vec2(ui.available_width(), 34.0)),
                                )
                                .on_hover_text(
                                    "Alle Reihen dieser Kette wieder als unabhängige 16-Step-Ketten anlegen",
                                )
                                .clicked()
                            {
                                split_chain = true;
                            }
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
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("SOUND-ZIEL")
                                    .color(theme::ORANGE)
                                    .size(10.0),
                            );
                            ui.checkbox(&mut chain.route.sound.enabled, "BANK + PROGRAM SENDEN");
                            if chain.route.sound.enabled {
                                ui.horizontal(|ui| {
                                    let mut enabled = chain.route.sound.bank_msb.is_some();
                                    if ui.checkbox(&mut enabled, "BANK MSB").changed() {
                                        chain.route.sound.bank_msb = enabled.then_some(0);
                                    }
                                    if let Some(value) = &mut chain.route.sound.bank_msb {
                                        ui.add(
                                            egui::DragValue::new(value)
                                                .range(0..=127)
                                                .speed(0.2),
                                        );
                                    }
                                });
                                ui.horizontal(|ui| {
                                    let mut enabled = chain.route.sound.bank_lsb.is_some();
                                    if ui.checkbox(&mut enabled, "BANK LSB").changed() {
                                        chain.route.sound.bank_lsb = enabled.then_some(0);
                                    }
                                    if let Some(value) = &mut chain.route.sound.bank_lsb {
                                        ui.add(
                                            egui::DragValue::new(value)
                                                .range(0..=127)
                                                .speed(0.2),
                                        );
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("PROGRAMM 1–128")
                                            .color(theme::MUTED)
                                            .size(9.0),
                                    );
                                    let mut displayed_program =
                                        u16::from(chain.route.sound.program) + 1;
                                    if ui
                                        .add(
                                            egui::DragValue::new(&mut displayed_program)
                                                .range(1..=128)
                                                .speed(0.2),
                                        )
                                        .changed()
                                    {
                                        chain.route.sound.program =
                                            displayed_program.saturating_sub(1) as u8;
                                    }
                                });
                                ui.checkbox(
                                    &mut chain.route.sound.send_on_connect,
                                    "BEIM VERBINDEN SENDEN",
                                );
                                if ui
                                    .add_enabled(
                                        midi_connected,
                                        egui::Button::new("SOUND JETZT SENDEN")
                                            .fill(Color32::from_rgb(58, 39, 22)),
                                    )
                                    .on_hover_text(
                                        "Bank Select MSB, Bank Select LSB und danach Program Change senden",
                                    )
                                    .clicked()
                                {
                                    send_sound = Some(chain_index);
                                }
                                ui.label(
                                    RichText::new(&sound_status)
                                        .color(theme::MUTED)
                                        .size(8.5),
                                );
                            }
                            ui.separator();
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
                        if link_with_next {
                            self.engine.stop();
                            self.midi.panic();
                            self.clear_live_skips_recorded();
                            match self.project.link_lane_with_next(self.selected_lane) {
                                Ok(()) => {
                                    self.project.active_scene = None;
                                    self.save_status = "REIHEN VERKETTET ✓".to_owned();
                                }
                                Err(error) => self.save_status = format!("KETTENFEHLER · {error}"),
                            }
                        }
                        if split_chain {
                            self.engine.stop();
                            self.midi.panic();
                            self.clear_live_skips_recorded();
                            match self.project.split_chain_for_lane(self.selected_lane) {
                                Ok(()) => {
                                    self.project.active_scene = None;
                                    self.save_status = "KETTE GETRENNT ✓".to_owned();
                                }
                                Err(error) => self.save_status = format!("KETTENFEHLER · {error}"),
                            }
                        }
                        if let Some(chain_index) = send_sound {
                            self.send_chain_sound(chain_index);
                        }
                        if render_arp && let Some(chain_index) = self.selected_chain_index() {
                            self.render_arpeggiator_pattern(chain_index);
                        }
                        if let Some(pattern_index) = switch_pattern
                            && let Some(chain_index) = self.selected_chain_index()
                        {
                            let performance_ids = self
                                .project
                                .chains
                                .get(chain_index)
                                .zip(self.project.patterns.get(pattern_index))
                                .map(|(chain, pattern)| (chain.id, pattern.id));
                            let _ = self.project.sync_active_pattern_from_lanes(chain_index);
                            if let Err(error) =
                                self.project.apply_chain_pattern(chain_index, pattern_index)
                            {
                                self.arp_status = format!("PATTERN-FEHLER · {error}");
                            } else {
                                self.project.active_scene = None;
                                self.midi.panic();
                                if let Some((chain_id, pattern_id)) = performance_ids {
                                    self.record_performance_action(
                                        PerformanceAction::PatternChange {
                                            chain_id,
                                            pattern_id,
                                        },
                                    );
                                }
                                self.clear_live_skips_recorded();
                            }
                        }

                        ui.add_space(12.0);
                        ui.separator();
                        ui.label(
                            RichText::new("Klaus hat die Clock.")
                                .italics()
                                .color(theme::MUTED),
                        );
                    });
            });
    }

    fn piano_roll(&mut self, ctx: &egui::Context) -> (f32, f32) {
        let max_height =
            (ctx.available_rect().height() - MATRIX_MIN_HEIGHT).max(PIANO_ROLL_MIN_HEIGHT);
        let default_height = PIANO_ROLL_DEFAULT_HEIGHT.min(max_height);
        let panel = egui::TopBottomPanel::bottom("piano_roll")
            .default_height(default_height)
            .min_height(PIANO_ROLL_MIN_HEIGHT)
            .max_height(max_height)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(11, 15, 17))
                    .inner_margin(10.0),
            )
            .show(ctx, |ui| {
                let lane_kind = self.project.lanes[self.selected_lane].kind;
                let scale_text = format!(
                    "{} {} · SCALE FOLD",
                    root_name(self.project.generation_plan.root),
                    scale_label(self.project.generation_plan.scale)
                );
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if lane_kind == LaneKind::Drums {
                            "DRUM PIANO ROLL"
                        } else {
                            "PIANO ROLL"
                        })
                        .strong(),
                    );
                    ui.label(RichText::new(scale_text).color(theme::MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("□ GROSS ÖFFNEN").clicked() {
                            self.piano_roll_window_open = true;
                        }
                    });
                });
                ui.separator();
                self.piano_roll_grid(ui, "piano_roll_panel_rows")
            });
        (panel.response.rect.height(), panel.inner)
    }

    fn piano_roll_grid(&mut self, ui: &mut egui::Ui, scroll_id: &'static str) -> f32 {
        let lane_kind = self.project.lanes[self.selected_lane].kind;
        let generator_config = &self.project.generation_plan.lane_configs[self.selected_lane];
        let (base_pitch, row_count) = if lane_kind == LaneKind::Drums {
            (36, 8_usize)
        } else {
            let note_minimum = self.project.lanes[self.selected_lane]
                .steps
                .iter()
                .flat_map(|step| step.notes.iter().map(|note| note.pitch))
                .min();
            let note_maximum = self.project.lanes[self.selected_lane]
                .steps
                .iter()
                .flat_map(|step| step.notes.iter().map(|note| note.pitch))
                .max();
            let (minimum, desired_maximum) = match (note_minimum, note_maximum) {
                (Some(minimum), Some(maximum)) => {
                    (minimum.saturating_sub(2), maximum.saturating_add(2))
                }
                _ => (generator_config.min_pitch, generator_config.max_pitch),
            };
            let minimum = minimum.min(116);
            let maximum = desired_maximum
                .max(minimum.saturating_add(11))
                .min(minimum.saturating_add(23))
                .min(127);
            (
                minimum,
                usize::from(maximum.saturating_sub(minimum) + 1).max(12),
            )
        };
        let cell_width =
            ((ui.available_width() - 72.0) / STEPS_PER_LANE as f32 - 3.0).clamp(22.0, 44.0);
        let rows = egui::ScrollArea::vertical()
            .id_salt(scroll_id)
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
                            let has_note = self.project.lanes[self.selected_lane].steps[step_index]
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
                                    Stroke::new(1.0_f32, Color32::WHITE)
                                } else {
                                    Stroke::new(1.0_f32, Color32::from_rgb(46, 57, 62))
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
        rows.inner_rect.height()
    }

    fn piano_roll_window(&mut self, ctx: &egui::Context) -> Option<f32> {
        if !self.piano_roll_window_open {
            return None;
        }
        let mut open = self.piano_roll_window_open;
        let lane_options: Vec<(usize, String)> = self
            .project
            .lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| (index, format!("{} · {}", index + 1, lane.name)))
            .collect();
        let lane_kind = self.project.lanes[self.selected_lane].kind;
        let scale_text = format!(
            "{} {} · SCALE FOLD",
            root_name(self.project.generation_plan.root),
            scale_label(self.project.generation_plan.scale)
        );
        let restore_rect = if self.piano_roll_fullscreen {
            None
        } else {
            self.piano_roll_restore_rect.take()
        };
        let mut window = egui::Window::new("▦ PIANO-ROLL-ARBEITSBEREICH")
            .open(&mut open)
            .default_width(1180.0)
            .default_height(720.0)
            .min_width(760.0)
            .min_height(420.0)
            .resizable(true)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(8, 13, 16)));
        if self.piano_roll_fullscreen {
            window = window.fixed_rect(ctx.screen_rect().shrink(8.0));
        } else if let Some(rect) = restore_rect {
            window = window.fixed_rect(rect);
        }
        let response = window.show(ctx, |ui| {
            let mut toggle_fullscreen = false;
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new(if lane_kind == LaneKind::Drums {
                        "DRUM PIANO ROLL"
                    } else {
                        "PIANO ROLL"
                    })
                    .color(theme::CYAN),
                );
                egui::ComboBox::from_id_salt("piano_roll_workspace_lane")
                    .selected_text(
                        lane_options
                            .get(self.selected_lane)
                            .map_or("REIHE", |(_, name)| name.as_str()),
                    )
                    .show_ui(ui, |ui| {
                        for (index, name) in &lane_options {
                            ui.selectable_value(&mut self.selected_lane, *index, name);
                        }
                    });
                ui.label(RichText::new(scale_text).color(theme::MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(if self.piano_roll_fullscreen {
                            "◱ FENSTER"
                        } else {
                            "□ VOLLBILD"
                        })
                        .clicked()
                    {
                        toggle_fullscreen = true;
                    }
                });
            });
            ui.separator();
            (
                self.piano_roll_grid(ui, "piano_roll_workspace_rows"),
                toggle_fullscreen,
            )
        });
        let mut viewport_height = None;
        let mut toggle_fullscreen = false;
        let mut current_rect = None;
        if let Some(response) = response {
            current_rect = Some(response.response.rect);
            if let Some((height, should_toggle_fullscreen)) = response.inner {
                viewport_height = Some(height);
                toggle_fullscreen = should_toggle_fullscreen;
            }
        }
        if toggle_fullscreen {
            if !self.piano_roll_fullscreen {
                self.piano_roll_restore_rect = current_rect;
            }
            self.piano_roll_fullscreen = !self.piano_roll_fullscreen;
        }
        if !open {
            self.piano_roll_fullscreen = false;
            self.piano_roll_restore_rect = None;
        }
        self.piano_roll_window_open = open;
        viewport_height
    }

    fn matrix(&mut self, ctx: &egui::Context) -> (f32, f32) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::GRAPHITE).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CHAIN MATRIX").color(theme::MUTED).size(9.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.mode == WorkMode::Perform {
                            ui.checkbox(&mut self.global_live_skip, "GLOBAL LIVE-SKIP");
                            if ui.button("LIVE-SKIPS LÖSCHEN").clicked() {
                                self.clear_live_skips_recorded();
                            }
                        } else {
                            ui.label(RichText::new("EDIT MODE").color(theme::CYAN).size(9.0));
                        }
                    });
                });
                ui.add_space(4.0);

                let matrix_rows = egui::ScrollArea::vertical()
                    .id_salt("chain_matrix_rows")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
                                            RichText::new(format!(
                                                "{}  {}",
                                                lane_index + 1,
                                                lane_name
                                            ))
                                            .color(if lane_selected {
                                                Color32::BLACK
                                            } else {
                                                color
                                            })
                                            .size(10.0),
                                        )
                                        .fill(if lane_selected { color } else { theme::PANEL }),
                                    )
                                    .clicked()
                                {
                                    self.selected_lane = lane_index;
                                }

                                let reserved = 66.0;
                                let step_size = ((ui.available_width() - reserved) / 16.0 - 4.0)
                                    .clamp(25.0, 46.0);
                                for step_index in 0..STEPS_PER_LANE {
                                    let step = &self.project.lanes[lane_index].steps[step_index];
                                    let live_skipped =
                                        self.live_skips.contains(&(lane_index, step_index));
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
                                            StepState::Active => color.gamma_multiply(
                                                0.38 + f32::from(step.probability) / 180.0,
                                            ),
                                            StepState::Rest => Color32::from_rgb(18, 24, 27),
                                            StepState::Skip => Color32::from_rgb(31, 25, 27),
                                        }
                                    };
                                    let stroke = if is_current {
                                        Stroke::new(2.5_f32, Color32::WHITE)
                                    } else if selected {
                                        Stroke::new(2.0_f32, theme::ACID)
                                    } else {
                                        Stroke::new(1.0_f32, Color32::from_rgb(49, 61, 66))
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
                                                let default_pitch = if lane_kind == LaneKind::Melody
                                                {
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
                                                        self.set_live_skip(
                                                            target_lane,
                                                            step_index,
                                                            should_add,
                                                        );
                                                    }
                                                } else {
                                                    let enabled = !self
                                                        .live_skips
                                                        .contains(&(lane_index, step_index));
                                                    self.set_live_skip(
                                                        lane_index, step_index, enabled,
                                                    );
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
                                        egui::Button::new("M").fill(mute_fill).stroke(Stroke::new(
                                            1.0_f32,
                                            Color32::from_rgb(63, 76, 82),
                                        )),
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
                                        egui::Button::new("S").fill(solo_fill).stroke(Stroke::new(
                                            1.0_f32,
                                            Color32::from_rgb(63, 76, 82),
                                        )),
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
                (matrix_rows.content_size.y, matrix_rows.inner_rect.height())
            })
            .inner
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
                        ui.label(
                            RichText::new(format!("v{} · PROTOTYPE", env!("CARGO_PKG_VERSION")))
                                .color(theme::MUTED),
                        );
                    });
                });
            });
        self.about_open = open;
    }
}

impl eframe::App for KlausApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_midi_events();
        let save_as = ctx.input(|input| {
            input.modifiers.command && input.modifiers.shift && input.key_pressed(egui::Key::S)
        });
        let save = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::S)
        });
        let open = ctx.input(|input| input.modifiers.command && input.key_pressed(egui::Key::O));
        let undo = ctx.input(|input| {
            input.modifiers.command && !input.modifiers.shift && input.key_pressed(egui::Key::Z)
        });
        let redo = ctx.input(|input| {
            input.modifiers.command
                && (input.key_pressed(egui::Key::Y)
                    || (input.modifiers.shift && input.key_pressed(egui::Key::Z)))
        });
        if save_as {
            self.save_project_as();
        } else if save {
            self.save_project();
        }
        if open {
            self.open_project();
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        self.capture_history_change();
        self.autosave_if_needed();
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame).as_secs_f64().min(0.1);
        self.last_frame = now;
        let transport_delta = self.advance_transport_clock(delta);
        if self.project.midi_clock_mode == MidiClockMode::Follower
            && self
                .last_external_clock
                .is_none_or(|last| last.elapsed() > Duration::from_secs(1))
        {
            self.clock_status = "WARTE AUF MIDI CLOCK".to_owned();
        }
        let arp_delta = if self.project.midi_clock_mode == MidiClockMode::Follower {
            transport_delta
        } else {
            delta
        };
        if self.update_arpeggiator(arp_delta) {
            ctx.request_repaint();
        }
        if self.arp_running {
            ctx.request_repaint_after(Duration::from_millis(2));
        }
        let triggers = self
            .engine
            .update(&self.project, &self.live_skips, transport_delta);
        if !triggers.is_empty() {
            self.schedule_midi(&triggers);
            ctx.request_repaint();
        }
        if self.engine.playing {
            ctx.request_repaint();
        }

        self.top_bar(ctx);
        self.project_bar(ctx);
        self.midi_bar(ctx);
        self.scenes_bar(ctx);
        self.piano_roll(ctx);
        self.inspector(ctx);
        self.matrix(ctx);
        self.piano_roll_window(ctx);
        self.generator_window(ctx);
        self.performance_window(ctx);
        self.composer_window(ctx);
        self.about_window(ctx);
    }
}

impl Drop for KlausApp {
    fn drop(&mut self) {
        self.project.sync_all_active_patterns();
        self.project.sync_active_performance_take();
        let _ = save_project_atomic(&self.project, &self.autosave_path);
    }
}

fn root_name(root: u8) -> &'static str {
    [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ][usize::from(root % 12)]
}

fn performance_position_label(pulse: u64) -> String {
    let bar = pulse / 96 + 1;
    let beat = (pulse % 96) / 24 + 1;
    let tick = pulse % 24;
    format!("TAKT {bar:02} · {beat}.{tick:02}")
}

fn performance_action_label(action: &PerformanceAction, project: &Project) -> String {
    match action {
        PerformanceAction::SceneLaunch { scene_id } => {
            let name = project
                .scenes
                .iter()
                .find(|scene| scene.id == *scene_id)
                .map_or("UNBEKANNTE SZENE", |scene| scene.name.as_str());
            format!("SZENE · {name}")
        }
        PerformanceAction::PatternChange {
            chain_id,
            pattern_id,
        } => {
            let chain = project
                .chains
                .iter()
                .find(|chain| chain.id == *chain_id)
                .map_or("UNBEKANNTE KETTE", |chain| chain.name.as_str());
            let pattern = project
                .patterns
                .iter()
                .find(|pattern| pattern.id == *pattern_id)
                .map_or("UNBEKANNTES PATTERN", |pattern| pattern.name.as_str());
            format!("PATTERN · {chain} · {pattern}")
        }
        PerformanceAction::LiveSkip {
            lane,
            step,
            enabled,
        } => format!(
            "LIVE-SKIP · REIHE {} · STEP {:02} · {}",
            lane + 1,
            step + 1,
            if *enabled { "AN" } else { "AUS" }
        ),
        PerformanceAction::TransportStart => "TRANSPORT · START".to_owned(),
        PerformanceAction::TransportStop => "TRANSPORT · STOPP".to_owned(),
    }
}

fn all_scales() -> [MusicalScale; 11] {
    [
        MusicalScale::Major,
        MusicalScale::NaturalMinor,
        MusicalScale::HarmonicMinor,
        MusicalScale::Dorian,
        MusicalScale::Phrygian,
        MusicalScale::Lydian,
        MusicalScale::Mixolydian,
        MusicalScale::MajorPentatonic,
        MusicalScale::MinorPentatonic,
        MusicalScale::WholeTone,
        MusicalScale::Chromatic,
    ]
}

fn scale_label(scale: MusicalScale) -> &'static str {
    match scale {
        MusicalScale::Major => "MAJOR",
        MusicalScale::NaturalMinor => "NATURAL MINOR",
        MusicalScale::HarmonicMinor => "HARMONIC MINOR",
        MusicalScale::Dorian => "DORIAN",
        MusicalScale::Phrygian => "PHRYGIAN",
        MusicalScale::Lydian => "LYDIAN",
        MusicalScale::Mixolydian => "MIXOLYDIAN",
        MusicalScale::MajorPentatonic => "MAJOR PENTATONIC",
        MusicalScale::MinorPentatonic => "MINOR PENTATONIC",
        MusicalScale::WholeTone => "WHOLE TONE",
        MusicalScale::Chromatic => "CHROMATIC",
    }
}

fn roles_for_kind(kind: LaneKind) -> &'static [GeneratorRole] {
    const MELODY: &[GeneratorRole] = &[
        GeneratorRole::BassPulse,
        GeneratorRole::Ostinato,
        GeneratorRole::Arpeggio,
        GeneratorRole::ChordPulse,
        GeneratorRole::CounterMelody,
        GeneratorRole::MelodicSatellite,
    ];
    const DRUMS: &[GeneratorRole] = &[GeneratorRole::DrumPulse, GeneratorRole::DrumPercussion];
    if kind == LaneKind::Drums {
        DRUMS
    } else {
        MELODY
    }
}

fn role_label(role: GeneratorRole) -> &'static str {
    match role {
        GeneratorRole::BassPulse => "BASS-PULS",
        GeneratorRole::Ostinato => "OSTINATO",
        GeneratorRole::Arpeggio => "ARPEGGIO",
        GeneratorRole::ChordPulse => "AKKORD-PULS",
        GeneratorRole::CounterMelody => "GEGENMELODIE",
        GeneratorRole::MelodicSatellite => "MELODISCHER SATELLIT",
        GeneratorRole::DrumPulse => "DRUM-PULS",
        GeneratorRole::DrumPercussion => "PERCUSSION-SATELLIT",
    }
}

fn generation_mode_label(mode: GenerationMode) -> &'static str {
    match mode {
        GenerationMode::Generate => "ERZEUGT",
        GenerationMode::Mutate => "MUTIERT",
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

fn clock_mode_label(mode: MidiClockMode) -> &'static str {
    match mode {
        MidiClockMode::Internal => "INTERNAL",
        MidiClockMode::Master => "MASTER",
        MidiClockMode::Follower => "FOLLOWER",
    }
}

fn scene_quantization_label(quantization: SceneLaunchQuantization) -> &'static str {
    match quantization {
        SceneLaunchQuantization::Immediate => "SOFORT",
        SceneLaunchQuantization::NextStep => "NÄCHSTER STEP",
        SceneLaunchQuantization::NextBeat => "NÄCHSTER BEAT",
        SceneLaunchQuantization::NextBar => "NÄCHSTER TAKT",
    }
}

fn quantization_boundary_crossed(previous: u64, current: u64, boundary: u64) -> bool {
    boundary > 0 && previous / boundary < current / boundary
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

    use super::{build_arp_sequence, performance_position_label, quantization_boundary_crossed};
    use crate::model::ArpMode;

    #[test]
    fn toolbar_panels_reserve_their_touch_control_height() {
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = super::KlausApp::new(&cc);
        // Keep Drop's autosave away from the user's project.
        let autosave =
            std::env::temp_dir().join(format!("klaus-layout-test-{}.json", std::process::id()));
        app.autosave_path = autosave.clone();
        app.project = crate::model::Project::demo();
        for width in [1024.0, 1440.0, 1920.0] {
            for system_theme in [eframe::egui::Theme::Light, eframe::egui::Theme::Dark] {
                let input = eframe::egui::RawInput {
                    screen_rect: Some(eframe::egui::Rect::from_min_size(
                        eframe::egui::Pos2::ZERO,
                        eframe::egui::vec2(width, 1120.0),
                    )),
                    system_theme: Some(system_theme),
                    ..Default::default()
                };
                // Include the first frame and subsequent stabilized frames.
                for _ in 0..3 {
                    let output = ctx.run(input.clone(), |ctx| {
                        app.top_bar(ctx);
                        app.project_bar(ctx);
                        app.midi_bar(ctx);
                        app.scenes_bar(ctx);
                        let mut bottom = 0.0;
                        for (id, minimum) in [
                            ("top_bar", 68.0),
                            ("project_bar", 52.0),
                            ("midi_bar", 102.0),
                            ("scenes_bar", 64.0),
                        ] {
                            let rect = eframe::egui::containers::panel::PanelState::load(
                                ctx,
                                eframe::egui::Id::new(id),
                            )
                            .unwrap()
                            .rect;
                            assert!(rect.height() >= minimum, "{id}: {rect:?}");
                            assert!(rect.top() >= bottom, "overlapping panel {id}");
                            bottom = rect.bottom();
                        }
                        assert!(ctx.available_rect().height() > 0.0);
                    });
                    for shape in output.shapes {
                        if let eframe::egui::epaint::Shape::Rect(rect) = shape.shape
                            && rect.fill == crate::theme::PANEL_RAISED
                        {
                            assert!(
                                shape.clip_rect.expand(0.5).contains_rect(rect.rect),
                                "toolbar control clipped at width {width}: {:?} by {:?}",
                                rect.rect,
                                shape.clip_rect
                            );
                        }
                    }
                }
            }
        }
        drop(app);
        let _ = std::fs::remove_file(autosave);
    }

    #[test]
    fn chain_matrix_scrolls_when_the_window_is_short() {
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = super::KlausApp::new(&cc);
        // Keep Drop's autosave away from the user's project.
        let autosave = std::env::temp_dir().join(format!(
            "klaus-matrix-layout-test-{}.json",
            std::process::id()
        ));
        app.autosave_path = autosave.clone();
        app.project = crate::model::Project::demo();

        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(1440.0, 720.0),
            )),
            system_theme: Some(eframe::egui::Theme::Dark),
            ..Default::default()
        };
        let mut matrix_size = (0.0, 0.0);
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                app.top_bar(ctx);
                app.project_bar(ctx);
                app.midi_bar(ctx);
                app.scenes_bar(ctx);
                app.piano_roll(ctx);
                app.inspector(ctx);
                matrix_size = app.matrix(ctx);
            });
        }

        let (content_height, viewport_height) = matrix_size;
        assert!(viewport_height > 0.0, "matrix viewport has no height");
        assert!(
            content_height > viewport_height,
            "matrix rows must overflow at 720 px: content={content_height}, viewport={viewport_height}"
        );

        drop(app);
        let _ = std::fs::remove_file(autosave);
    }

    #[test]
    fn piano_roll_honors_a_resized_height_and_exposes_more_rows() {
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = super::KlausApp::new(&cc);
        // Keep Drop's autosave away from the user's project.
        let autosave = std::env::temp_dir().join(format!(
            "klaus-piano-roll-layout-test-{}.json",
            std::process::id()
        ));
        app.autosave_path = autosave.clone();
        app.project = crate::model::Project::demo();

        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(1440.0, 1120.0),
            )),
            system_theme: Some(eframe::egui::Theme::Dark),
            ..Default::default()
        };
        let mut default_size = (0.0, 0.0);
        let _ = ctx.run(input.clone(), |ctx| {
            app.top_bar(ctx);
            app.project_bar(ctx);
            app.midi_bar(ctx);
            app.scenes_bar(ctx);
            default_size = app.piano_roll(ctx);
        });

        let requested_height = 480.0;
        let mut resized_size = (0.0, 0.0);
        let _ = ctx.run(input, |ctx| {
            app.top_bar(ctx);
            app.project_bar(ctx);
            app.midi_bar(ctx);
            app.scenes_bar(ctx);
            ctx.data_mut(|data| {
                data.insert_persisted(
                    eframe::egui::Id::new("piano_roll"),
                    eframe::egui::containers::panel::PanelState {
                        rect: eframe::egui::Rect::from_min_size(
                            eframe::egui::Pos2::ZERO,
                            eframe::egui::vec2(1440.0, requested_height),
                        ),
                    },
                );
            });
            resized_size = app.piano_roll(ctx);
        });

        assert!(
            default_size.1 >= 4.0 * 44.0,
            "default piano roll must expose at least four touch rows: {default_size:?}"
        );
        assert!(
            (resized_size.0 - requested_height).abs() <= 1.0,
            "piano roll ignored stored resize: {resized_size:?}"
        );
        assert!(
            resized_size.1 > default_size.1 + 100.0,
            "larger panel did not expose more row space: default={default_size:?}, resized={resized_size:?}"
        );

        drop(app);
        let _ = std::fs::remove_file(autosave);
    }

    #[test]
    fn piano_roll_workspace_opens_with_a_large_editing_area() {
        let ctx = eframe::egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = super::KlausApp::new(&cc);
        // Keep Drop's autosave away from the user's project.
        let autosave = std::env::temp_dir().join(format!(
            "klaus-piano-roll-workspace-test-{}.json",
            std::process::id()
        ));
        app.autosave_path = autosave.clone();
        app.project = crate::model::Project::demo();
        app.piano_roll_window_open = true;

        let input = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::Pos2::ZERO,
                eframe::egui::vec2(1440.0, 1000.0),
            )),
            system_theme: Some(eframe::egui::Theme::Dark),
            ..Default::default()
        };
        let mut viewport_height = 0.0;
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                viewport_height = app.piano_roll_window(ctx).unwrap_or_default();
            });
        }
        let windowed_viewport_height = viewport_height;

        app.piano_roll_fullscreen = true;
        for _ in 0..2 {
            let _ = ctx.run(input.clone(), |ctx| {
                viewport_height = app.piano_roll_window(ctx).unwrap_or_default();
            });
        }

        assert!(app.piano_roll_window_open);
        assert!(
            windowed_viewport_height >= 500.0,
            "piano-roll workspace is too small: {windowed_viewport_height}"
        );
        assert!(
            viewport_height >= windowed_viewport_height + 200.0,
            "fullscreen did not enlarge the editing area: windowed={windowed_viewport_height}, fullscreen={viewport_height}"
        );

        drop(app);
        let _ = std::fs::remove_file(autosave);
    }

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

    #[test]
    fn scene_quantization_only_fires_when_boundary_is_crossed() {
        assert!(!quantization_boundary_crossed(23, 23, 24));
        assert!(quantization_boundary_crossed(23, 24, 24));
        assert!(!quantization_boundary_crossed(24, 47, 24));
        assert!(quantization_boundary_crossed(95, 97, 96));
    }

    #[test]
    fn performance_positions_use_24_ppqn_and_four_four_bars() {
        assert_eq!(performance_position_label(0), "TAKT 01 · 1.00");
        assert_eq!(performance_position_label(24), "TAKT 01 · 2.00");
        assert_eq!(performance_position_label(95), "TAKT 01 · 4.23");
        assert_eq!(performance_position_label(96), "TAKT 02 · 1.00");
    }
}
