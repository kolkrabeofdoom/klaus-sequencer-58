use std::collections::BTreeSet;

use crate::model::{
    Chain, Direction, LANE_COUNT, Project, STEPS_PER_LANE, Step, StepDivision, StepState,
};

#[derive(Debug, Clone)]
pub struct PreviewEngine {
    pub playing: bool,
    pub lane_positions: [usize; LANE_COUNT],
    pub lane_active: [bool; LANE_COUNT],
    runtimes: Vec<ChainRuntime>,
    emit_current_step: bool,
}

#[derive(Debug, Clone)]
struct ChainRuntime {
    flat_position: usize,
    elapsed_seconds: f64,
    ping_pong_forward: bool,
    advances: u64,
    random_state: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepTrigger {
    pub chain: usize,
    pub lane: usize,
    pub step: usize,
    pub cycle: u64,
}

impl Default for PreviewEngine {
    fn default() -> Self {
        Self {
            playing: false,
            lane_positions: [0; LANE_COUNT],
            lane_active: [false; LANE_COUNT],
            runtimes: Vec::new(),
            emit_current_step: false,
        }
    }
}

impl PreviewEngine {
    pub fn toggle(&mut self) {
        if !self.playing {
            self.emit_current_step = true;
        }
        self.playing = !self.playing;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.lane_positions = [0; LANE_COUNT];
        self.lane_active = [false; LANE_COUNT];
        self.runtimes.clear();
        self.emit_current_step = false;
    }

    pub fn update(
        &mut self,
        project: &Project,
        live_skips: &BTreeSet<(usize, usize)>,
        delta_seconds: f64,
    ) -> Vec<StepTrigger> {
        self.sync_runtimes(project, live_skips);
        if !self.playing {
            return Vec::new();
        }

        let mut triggers = Vec::new();
        if self.emit_current_step {
            for (chain_index, runtime) in self.runtimes.iter().enumerate() {
                if let Some(trigger) = trigger_at(project, chain_index, runtime) {
                    triggers.push(trigger);
                }
            }
            self.emit_current_step = false;
        }

        for (chain_index, chain) in project.chains.iter().enumerate() {
            let Some(runtime) = self.runtimes.get_mut(chain_index) else {
                continue;
            };
            runtime.elapsed_seconds += delta_seconds.max(0.0);
            loop {
                let duration = swung_step_seconds(
                    project.bpm,
                    chain.division,
                    chain.swing,
                    runtime.flat_position,
                );
                if runtime.elapsed_seconds < duration {
                    break;
                }
                runtime.elapsed_seconds -= duration;
                if advance_runtime(runtime, project, chain, live_skips).is_some()
                    && let Some(trigger) = trigger_at(project, chain_index, runtime)
                {
                    triggers.push(trigger);
                }
            }
        }

        self.refresh_lane_indicators(project);
        triggers
    }

    fn sync_runtimes(&mut self, project: &Project, live_skips: &BTreeSet<(usize, usize)>) {
        if self.runtimes.len() == project.chains.len() {
            self.refresh_lane_indicators(project);
            return;
        }
        self.runtimes = project
            .chains
            .iter()
            .enumerate()
            .map(|(index, chain)| {
                let start = initial_position(project, chain, live_skips);
                ChainRuntime {
                    flat_position: start,
                    elapsed_seconds: 0.0,
                    ping_pong_forward: chain.direction != Direction::Reverse,
                    advances: 0,
                    random_state: 58_u64
                        .wrapping_add(index as u64)
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15),
                }
            })
            .collect();
        self.refresh_lane_indicators(project);
    }

    fn refresh_lane_indicators(&mut self, project: &Project) {
        self.lane_active = [false; LANE_COUNT];
        for (chain, runtime) in project.chains.iter().zip(&self.runtimes) {
            if let Some((lane, step)) = flat_to_lane_step(chain, runtime.flat_position)
                && lane < LANE_COUNT
            {
                self.lane_positions[lane] = step;
                self.lane_active[lane] = true;
            }
        }
    }
}

pub fn division_seconds(bpm: f32, division: StepDivision) -> f64 {
    let beat = 60.0 / f64::from(bpm.max(20.0));
    match division {
        StepDivision::Quarter => beat,
        StepDivision::Eighth => beat / 2.0,
        StepDivision::EighthTriplet => beat / 3.0,
        StepDivision::Sixteenth => beat / 4.0,
        StepDivision::SixteenthTriplet => beat / 6.0,
        StepDivision::ThirtySecond => beat / 8.0,
    }
}

pub fn swung_step_seconds(bpm: f32, division: StepDivision, swing: f32, position: usize) -> f64 {
    let base = division_seconds(bpm, division);
    let swing = f64::from(swing.clamp(0.5, 0.75));
    if position.is_multiple_of(2) {
        base * 2.0 * swing
    } else {
        base * 2.0 * (1.0 - swing)
    }
}

pub fn next_timed_index(steps: &[Step], current: usize) -> usize {
    if steps.is_empty() {
        return 0;
    }
    for offset in 1..=steps.len() {
        let candidate = (current + offset) % steps.len();
        if steps[candidate].state != StepState::Skip {
            return candidate;
        }
    }
    current.min(steps.len() - 1)
}

pub fn probability_hits(seed: u64, lane: usize, step: usize, cycle: u64, probability: u8) -> bool {
    if probability >= 100 {
        return true;
    }
    if probability == 0 {
        return false;
    }
    let mut value = seed
        ^ (lane as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (step as u64).rotate_left(17)
        ^ cycle.rotate_left(33);
    value ^= value >> 12;
    value ^= value << 25;
    value ^= value >> 27;
    let roll = value.wrapping_mul(0x2545_F491_4F6C_DD1D) % 100;
    roll < u64::from(probability)
}

fn initial_position(
    project: &Project,
    chain: &Chain,
    live_skips: &BTreeSet<(usize, usize)>,
) -> usize {
    let length = chain.lane_ids.len() * STEPS_PER_LANE;
    if length == 0 {
        return 0;
    }
    let positions: Vec<usize> = if chain.direction == Direction::Reverse {
        (0..length).rev().collect()
    } else {
        (0..length).collect()
    };
    positions
        .into_iter()
        .find(|position| !position_is_skipped(project, chain, *position, live_skips))
        .unwrap_or(0)
}

fn advance_runtime(
    runtime: &mut ChainRuntime,
    project: &Project,
    chain: &Chain,
    live_skips: &BTreeSet<(usize, usize)>,
) -> Option<usize> {
    let length = chain.lane_ids.len() * STEPS_PER_LANE;
    if length == 0 {
        return None;
    }

    let next = match chain.direction {
        Direction::Forward => (1..=length)
            .map(|offset| (runtime.flat_position + offset) % length)
            .find(|position| !position_is_skipped(project, chain, *position, live_skips)),
        Direction::Reverse => (1..=length)
            .map(|offset| (runtime.flat_position + length - (offset % length)) % length)
            .find(|position| !position_is_skipped(project, chain, *position, live_skips)),
        Direction::PingPong => {
            let mut candidate = runtime.flat_position;
            let mut found = None;
            for _ in 0..(length.saturating_mul(2).max(1)) {
                if runtime.ping_pong_forward {
                    if candidate + 1 >= length {
                        runtime.ping_pong_forward = false;
                        candidate = candidate.saturating_sub(1);
                    } else {
                        candidate += 1;
                    }
                } else if candidate == 0 {
                    runtime.ping_pong_forward = true;
                    candidate = (candidate + 1).min(length - 1);
                } else {
                    candidate -= 1;
                }
                if !position_is_skipped(project, chain, candidate, live_skips) {
                    found = Some(candidate);
                    break;
                }
            }
            found
        }
        Direction::Random => {
            let playable: Vec<usize> = (0..length)
                .filter(|position| !position_is_skipped(project, chain, *position, live_skips))
                .collect();
            if playable.is_empty() {
                None
            } else {
                runtime.random_state ^= runtime.random_state >> 12;
                runtime.random_state ^= runtime.random_state << 25;
                runtime.random_state ^= runtime.random_state >> 27;
                let index = runtime.random_state.wrapping_mul(0x2545_F491_4F6C_DD1D) as usize
                    % playable.len();
                Some(playable[index])
            }
        }
    };

    if let Some(position) = next {
        runtime.flat_position = position;
        runtime.advances = runtime.advances.wrapping_add(1);
    }
    next
}

fn trigger_at(
    project: &Project,
    chain_index: usize,
    runtime: &ChainRuntime,
) -> Option<StepTrigger> {
    let chain = project.chains.get(chain_index)?;
    let (lane, step) = flat_to_lane_step(chain, runtime.flat_position)?;
    Some(StepTrigger {
        chain: chain_index,
        lane,
        step,
        cycle: runtime.advances / (chain.lane_ids.len() * STEPS_PER_LANE).max(1) as u64,
    })
}

fn flat_to_lane_step(chain: &Chain, flat_position: usize) -> Option<(usize, usize)> {
    let lane_offset = flat_position / STEPS_PER_LANE;
    let lane = *chain.lane_ids.get(lane_offset)?;
    Some((lane, flat_position % STEPS_PER_LANE))
}

fn position_is_skipped(
    project: &Project,
    chain: &Chain,
    flat_position: usize,
    live_skips: &BTreeSet<(usize, usize)>,
) -> bool {
    let Some((lane, step)) = flat_to_lane_step(chain, flat_position) else {
        return true;
    };
    live_skips.contains(&(lane, step))
        || project
            .lanes
            .get(lane)
            .and_then(|lane| lane.steps.get(step))
            .is_none_or(|step| step.state == StepState::Skip)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{PreviewEngine, division_seconds, swung_step_seconds};
    use crate::model::{Direction, Project, StepDivision};

    #[test]
    fn chained_rows_play_sequentially_instead_of_in_parallel() {
        let project = Project::demo();
        let mut engine = PreviewEngine::default();
        engine.toggle();
        let initial = engine.update(&project, &BTreeSet::new(), 0.0);
        assert!(
            initial
                .iter()
                .any(|trigger| trigger.chain == 0 && trigger.lane == 0)
        );
        assert!(
            !initial
                .iter()
                .any(|trigger| trigger.chain == 0 && trigger.lane == 1)
        );

        let step = division_seconds(project.bpm, StepDivision::Sixteenth);
        let later = engine.update(&project, &BTreeSet::new(), step * 16.1);
        assert!(
            later
                .iter()
                .any(|trigger| trigger.chain == 0 && trigger.lane == 1)
        );
    }

    #[test]
    fn reverse_direction_starts_at_the_end_of_a_chain() {
        let mut project = Project::demo();
        project.chains[0].direction = Direction::Reverse;
        let mut engine = PreviewEngine::default();
        engine.toggle();
        let triggers = engine.update(&project, &BTreeSet::new(), 0.0);
        assert!(
            triggers
                .iter()
                .any(|trigger| trigger.chain == 0 && trigger.lane == 1 && trigger.step == 15)
        );
    }

    #[test]
    fn swing_preserves_the_duration_of_a_step_pair() {
        let straight = division_seconds(120.0, StepDivision::Sixteenth);
        let first = swung_step_seconds(120.0, StepDivision::Sixteenth, 0.62, 0);
        let second = swung_step_seconds(120.0, StepDivision::Sixteenth, 0.62, 1);
        assert!((first + second - straight * 2.0).abs() < 0.000_001);
        assert!(first > second);
    }
}
