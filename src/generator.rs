use crate::model::{
    GenerationPlan, GeneratorRole, LaneGeneratorConfig, LaneKind, NoteEvent, STEPS_PER_LANE, Step,
    StepState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationMode {
    Generate,
    Mutate,
}

pub fn generate_lane(
    plan: &GenerationPlan,
    lane_index: usize,
    kind: LaneKind,
    current: &[Step],
    mode: GenerationMode,
) -> Vec<Step> {
    let config = plan
        .lane_configs
        .get(lane_index)
        .cloned()
        .unwrap_or_else(|| LaneGeneratorConfig::for_lane(lane_index, kind));
    let seed = plan
        .seed
        .wrapping_add(config.seed_offset)
        .wrapping_add((lane_index as u64 + 1).wrapping_mul(0x9E37_79B9));
    let candidate = generate_candidate(plan, &config, kind, seed);
    let generated = match mode {
        GenerationMode::Generate => candidate,
        GenerationMode::Mutate => mutate_steps(current, &candidate, &config, seed ^ 0x58_58_58),
    };
    apply_locks(current, &generated, &config)
}

fn generate_candidate(
    plan: &GenerationPlan,
    config: &LaneGeneratorConfig,
    kind: LaneKind,
    seed: u64,
) -> Vec<Step> {
    let mut rng = KlausRng::new(seed);
    let pool = note_pool(plan, config, kind);
    let motif = build_motif(&pool, config, &mut rng);
    let effective_length = usize::from(config.effective_length.clamp(1, STEPS_PER_LANE as u8));
    let mut steps = Vec::with_capacity(STEPS_PER_LANE);
    let mut active_counter = 0_usize;

    for step_index in 0..STEPS_PER_LANE {
        if step_index >= effective_length {
            let mut step = Step::rest();
            step.state = StepState::Skip;
            steps.push(step);
            continue;
        }

        if !step_is_active(config, step_index, &mut rng) {
            steps.push(Step::rest());
            continue;
        }

        let repeat = active_counter / motif.len().max(1);
        let mut pitch = motif[active_counter % motif.len().max(1)];
        if repeat > 0 && rng.chance(config.drift) {
            pitch = drift_pitch(pitch, &pool, config, &mut rng);
        }
        active_counter += 1;

        let pitches = if kind == LaneKind::Drums {
            vec![drum_pitch(config.role, step_index, &mut rng)]
        } else if config.role == GeneratorRole::ChordPulse {
            chord_pitches(pitch, &pool, config.heat)
        } else {
            vec![pitch]
        };

        let accent = step_index.is_multiple_of(4);
        let velocity_base = if accent { 108 } else { 82 };
        let velocity = (velocity_base + rng.range_u8(0, 18)).min(127);
        let gate = gate_for_role(config.role, &mut rng);
        let notes = pitches
            .into_iter()
            .map(|pitch| NoteEvent {
                pitch,
                velocity,
                gate,
            })
            .collect();
        let ratchets = if rng.chance(config.heat.saturating_sub(8)) {
            let maximum = if matches!(
                config.role,
                GeneratorRole::Arpeggio | GeneratorRole::DrumPercussion
            ) {
                4
            } else {
                3
            };
            rng.range_u8(2, maximum)
        } else {
            1
        };
        let probability = if rng.chance(config.drift / 2) {
            rng.range_u8(68, 96)
        } else {
            100
        };
        steps.push(Step {
            state: StepState::Active,
            notes,
            probability,
            ratchets,
        });
    }

    if !steps[..effective_length]
        .iter()
        .any(|step| step.state == StepState::Active)
    {
        let pitch = if kind == LaneKind::Drums {
            36
        } else {
            *motif.first().unwrap_or(&config.min_pitch.min(127))
        };
        steps[0] = Step::active_with_pitch(pitch);
    }
    steps
}

fn note_pool(plan: &GenerationPlan, config: &LaneGeneratorConfig, kind: LaneKind) -> Vec<u8> {
    let minimum = config.min_pitch.min(config.max_pitch).min(127);
    let maximum = config.max_pitch.max(config.min_pitch).min(127);
    if kind == LaneKind::Drums {
        return (minimum..=maximum).collect();
    }
    let mut pool: Vec<u8> = (minimum..=maximum)
        .filter(|pitch| plan.scale.contains(plan.root, *pitch))
        .collect();
    if pool.is_empty() {
        pool.push(minimum);
    }
    pool
}

fn build_motif(pool: &[u8], config: &LaneGeneratorConfig, rng: &mut KlausRng) -> Vec<u8> {
    let length = usize::from(config.motif_length.clamp(1, 11));
    let mut motif = Vec::with_capacity(length);
    let lower_span = (pool.len() / 3).max(1);
    let mut index = match config.role {
        GeneratorRole::BassPulse => rng.range_usize(0, lower_span.saturating_sub(1)),
        GeneratorRole::Arpeggio | GeneratorRole::ChordPulse => pool.len() / 3,
        _ => rng.range_usize(0, pool.len().saturating_sub(1)),
    };
    let mut direction = if rng.chance(50) { 1_i32 } else { -1_i32 };

    for position in 0..length {
        motif.push(pool[index.min(pool.len() - 1)]);
        if position + 1 == length {
            break;
        }
        if rng.chance(100_u8.saturating_sub(config.order)) {
            direction = if rng.chance(50) { 1 } else { -1 };
        }
        let max_jump = 1 + usize::from(config.movement / 20);
        let first = rng.range_usize(0, max_jump);
        let second = rng.range_usize(0, max_jump);
        let mut jump = first.min(second);
        if config.role == GeneratorRole::Arpeggio && position.is_multiple_of(2) {
            jump = jump.max(2).min(max_jump);
        }
        if config.role == GeneratorRole::BassPulse {
            jump = jump.min(2);
        }
        let candidate = index as i32 + direction * jump as i32;
        if candidate < 0 || candidate >= pool.len() as i32 {
            direction *= -1;
        }
        index = (index as i32 + direction * jump as i32)
            .clamp(0, pool.len().saturating_sub(1) as i32) as usize;
    }
    motif
}

fn step_is_active(config: &LaneGeneratorConfig, step: usize, rng: &mut KlausRng) -> bool {
    let density = config.density.clamp(1, 100);
    match config.role {
        GeneratorRole::BassPulse => step.is_multiple_of(4) || rng.chance(density),
        GeneratorRole::Ostinato | GeneratorRole::Arpeggio => {
            step == 0 || rng.chance(density.saturating_add(8).min(100))
        }
        GeneratorRole::ChordPulse => {
            step.is_multiple_of(4) && rng.chance(density.saturating_add(35).min(100))
        }
        GeneratorRole::CounterMelody | GeneratorRole::MelodicSatellite => {
            step == 0 || rng.chance(density.saturating_sub(5))
        }
        GeneratorRole::DrumPulse => {
            step.is_multiple_of(4) || (step % 4 == 2 && rng.chance(density / 2))
        }
        GeneratorRole::DrumPercussion => {
            (step % 2 == 1 && rng.chance(density.saturating_add(15).min(100)))
                || rng.chance(density / 3)
        }
    }
}

fn drift_pitch(pitch: u8, pool: &[u8], config: &LaneGeneratorConfig, rng: &mut KlausRng) -> u8 {
    let index = pool
        .iter()
        .position(|candidate| *candidate == pitch)
        .unwrap_or(0);
    let distance = 1 + usize::from(config.movement / 35);
    let offset = rng.range_usize(1, distance.max(1));
    let candidate = if rng.chance(50) {
        index.saturating_add(offset).min(pool.len() - 1)
    } else {
        index.saturating_sub(offset)
    };
    pool[candidate]
}

fn chord_pitches(root: u8, pool: &[u8], heat: u8) -> Vec<u8> {
    let root_index = pool.iter().position(|pitch| *pitch == root).unwrap_or(0);
    let mut notes = vec![root];
    for offset in [2_usize, 4] {
        if let Some(pitch) = pool.get(root_index + offset) {
            notes.push(*pitch);
        }
    }
    if heat >= 72
        && let Some(pitch) = pool.get(root_index + 6)
    {
        notes.push(*pitch);
    }
    notes.sort_unstable();
    notes.dedup();
    notes
}

fn drum_pitch(role: GeneratorRole, step: usize, rng: &mut KlausRng) -> u8 {
    match role {
        GeneratorRole::DrumPulse => {
            if step.is_multiple_of(4) {
                36
            } else if step % 4 == 2 {
                38
            } else {
                42
            }
        }
        _ => [39, 40, 41, 42, 43][rng.range_usize(0, 4)],
    }
}

fn gate_for_role(role: GeneratorRole, rng: &mut KlausRng) -> f32 {
    let base = match role {
        GeneratorRole::BassPulse => 0.68,
        GeneratorRole::Ostinato => 0.62,
        GeneratorRole::Arpeggio => 0.48,
        GeneratorRole::ChordPulse => 0.9,
        GeneratorRole::CounterMelody | GeneratorRole::MelodicSatellite => 0.78,
        GeneratorRole::DrumPulse | GeneratorRole::DrumPercussion => 0.32,
    };
    (base + f32::from(rng.range_u8(0, 12)) / 100.0).min(0.98)
}

fn mutate_steps(
    current: &[Step],
    candidate: &[Step],
    config: &LaneGeneratorConfig,
    seed: u64,
) -> Vec<Step> {
    let mut rng = KlausRng::new(seed);
    candidate
        .iter()
        .enumerate()
        .map(|(index, generated)| {
            if rng.chance(config.mutation) {
                generated.clone()
            } else {
                current.get(index).cloned().unwrap_or_else(Step::rest)
            }
        })
        .collect()
}

fn apply_locks(current: &[Step], generated: &[Step], config: &LaneGeneratorConfig) -> Vec<Step> {
    generated
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let Some(existing) = current.get(index) else {
                return candidate.clone();
            };
            let mut step = candidate.clone();
            if config.lock_rhythm {
                step.state = existing.state;
            }
            if config.lock_notes {
                step.notes = existing.notes.clone();
            }
            if config.lock_expression {
                step.probability = existing.probability;
                step.ratchets = existing.ratchets;
                let old_expression = existing
                    .notes
                    .first()
                    .map(|note| (note.velocity, note.gate));
                if let Some((velocity, gate)) = old_expression {
                    for note in &mut step.notes {
                        note.velocity = velocity;
                        note.gate = gate;
                    }
                }
            }
            step
        })
        .collect()
}

struct KlausRng {
    state: u64,
}

impl KlausRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed.max(1) ^ 0x4B4C_4155_5358_3538,
        }
    }

    fn next(&mut self) -> u64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        self.state = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        self.state
    }

    fn chance(&mut self, percent: u8) -> bool {
        self.next() % 100 < u64::from(percent.min(100))
    }

    fn range_u8(&mut self, minimum: u8, maximum: u8) -> u8 {
        if minimum >= maximum {
            return minimum;
        }
        minimum + (self.next() % u64::from(maximum - minimum + 1)) as u8
    }

    fn range_usize(&mut self, minimum: usize, maximum: usize) -> usize {
        if minimum >= maximum {
            return minimum;
        }
        minimum + self.next() as usize % (maximum - minimum + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::{GenerationMode, generate_lane};
    use crate::model::{
        GenerationPlan, GeneratorRole, LaneKind, MusicalScale, STEPS_PER_LANE, StepState,
    };

    fn melody_plan() -> GenerationPlan {
        GenerationPlan {
            lane_configs: vec![crate::model::LaneGeneratorConfig::for_lane(
                0,
                LaneKind::Melody,
            )],
            ..GenerationPlan::default()
        }
    }

    #[test]
    fn generation_is_deterministic_for_the_same_seed() {
        let plan = melody_plan();
        let current = vec![crate::model::Step::rest(); STEPS_PER_LANE];
        assert_eq!(
            generate_lane(
                &plan,
                0,
                LaneKind::Melody,
                &current,
                GenerationMode::Generate
            ),
            generate_lane(
                &plan,
                0,
                LaneKind::Melody,
                &current,
                GenerationMode::Generate
            )
        );
    }

    #[test]
    fn melody_notes_stay_inside_scale_and_range() {
        let mut plan = melody_plan();
        assert_eq!(plan.root, 2);
        assert_eq!(plan.scale, MusicalScale::Dorian);
        plan.lane_configs[0].min_pitch = 43;
        plan.lane_configs[0].max_pitch = 67;
        let steps = generate_lane(
            &plan,
            0,
            LaneKind::Melody,
            &vec![crate::model::Step::rest(); STEPS_PER_LANE],
            GenerationMode::Generate,
        );
        for note in steps.iter().flat_map(|step| &step.notes) {
            assert!((43..=67).contains(&note.pitch));
            assert!(plan.scale.contains(plan.root, note.pitch));
        }
    }

    #[test]
    fn effective_length_is_encoded_with_skips() {
        let mut plan = melody_plan();
        plan.lane_configs[0].effective_length = 13;
        let steps = generate_lane(
            &plan,
            0,
            LaneKind::Melody,
            &vec![crate::model::Step::rest(); STEPS_PER_LANE],
            GenerationMode::Generate,
        );
        assert_eq!(
            steps
                .iter()
                .filter(|step| step.state != StepState::Skip)
                .count(),
            13
        );
    }

    #[test]
    fn chord_role_produces_polyphonic_steps() {
        let mut plan = melody_plan();
        plan.lane_configs[0].role = GeneratorRole::ChordPulse;
        let steps = generate_lane(
            &plan,
            0,
            LaneKind::Melody,
            &vec![crate::model::Step::rest(); STEPS_PER_LANE],
            GenerationMode::Generate,
        );
        assert!(steps.iter().any(|step| step.notes.len() >= 2));
    }

    #[test]
    fn locks_preserve_existing_domains() {
        let mut plan = melody_plan();
        plan.lane_configs[0].lock_notes = true;
        plan.lane_configs[0].lock_rhythm = true;
        plan.lane_configs[0].lock_expression = true;
        let mut current = vec![crate::model::Step::rest(); STEPS_PER_LANE];
        current[3] = crate::model::Step::active_with_pitch(61);
        current[3].probability = 73;
        current[3].ratchets = 4;
        let generated = generate_lane(
            &plan,
            0,
            LaneKind::Melody,
            &current,
            GenerationMode::Generate,
        );
        assert_eq!(generated, current);
    }
}
