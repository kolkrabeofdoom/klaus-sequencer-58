use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};

const COMMAND_QUEUE_SIZE: usize = 4096;
const IDLE_POLL: Duration = Duration::from_millis(100);
const MAX_SCHEDULER_SLEEP: Duration = Duration::from_millis(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiStatus {
    Disconnected,
    Connecting(String),
    Connected(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiEvent {
    Ports(Vec<String>),
    Status(MidiStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiInputMessage {
    pub timestamp_micros: u64,
    pub data: [u8; 3],
    pub len: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiInputEvent {
    Ports(Vec<String>),
    Status(MidiStatus),
    Message(MidiInputMessage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteInput {
    On {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
    Off {
        channel: u8,
        pitch: u8,
    },
}

enum MidiCommand {
    RefreshPorts,
    Connect(String),
    Disconnect,
    Schedule { at: Instant, data: [u8; 3] },
    Panic,
    Shutdown,
}

enum MidiInputCommand {
    RefreshPorts,
    Connect(String),
    Disconnect,
    Shutdown,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ScheduledMessage {
    at: Instant,
    order: u64,
    data: [u8; 3],
}

impl Ord for ScheduledMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at
            .cmp(&self.at)
            .then_with(|| other.order.cmp(&self.order))
    }
}

impl PartialOrd for ScheduledMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub struct MidiService {
    command_tx: SyncSender<MidiCommand>,
    event_rx: Receiver<MidiEvent>,
    worker: Option<JoinHandle<()>>,
}

pub struct MidiInputService {
    command_tx: SyncSender<MidiInputCommand>,
    event_rx: Receiver<MidiInputEvent>,
    worker: Option<JoinHandle<()>>,
}

impl MidiService {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(COMMAND_QUEUE_SIZE);
        let (event_tx, event_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("klaus-midi-scheduler".to_owned())
            .spawn(move || worker_loop(command_rx, event_tx))
            .expect("MIDI-Arbeitsthread muss gestartet werden können");
        let service = Self {
            command_tx,
            event_rx,
            worker: Some(worker),
        };
        service.refresh_ports();
        service
    }

    pub fn refresh_ports(&self) {
        let _ = self.command_tx.try_send(MidiCommand::RefreshPorts);
    }

    pub fn connect(&self, port_name: String) {
        let _ = self.command_tx.try_send(MidiCommand::Connect(port_name));
    }

    pub fn disconnect(&self) {
        let _ = self.command_tx.try_send(MidiCommand::Disconnect);
    }

    pub fn schedule(&self, at: Instant, data: [u8; 3]) {
        let _ = self.command_tx.try_send(MidiCommand::Schedule { at, data });
    }

    pub fn panic(&self) {
        let _ = self.command_tx.try_send(MidiCommand::Panic);
    }

    pub fn poll_event(&self) -> Option<MidiEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Default for MidiService {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiInputService {
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::sync_channel(64);
        let (event_tx, event_rx) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("klaus-midi-input".to_owned())
            .spawn(move || input_worker_loop(command_rx, event_tx))
            .expect("MIDI-Eingangsthread muss gestartet werden können");
        let service = Self {
            command_tx,
            event_rx,
            worker: Some(worker),
        };
        service.refresh_ports();
        service
    }

    pub fn refresh_ports(&self) {
        let _ = self.command_tx.try_send(MidiInputCommand::RefreshPorts);
    }

    pub fn connect(&self, port_name: String) {
        let _ = self
            .command_tx
            .try_send(MidiInputCommand::Connect(port_name));
    }

    pub fn disconnect(&self) {
        let _ = self.command_tx.try_send(MidiInputCommand::Disconnect);
    }

    pub fn poll_event(&self) -> Option<MidiInputEvent> {
        self.event_rx.try_recv().ok()
    }
}

impl Default for MidiInputService {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MidiInputService {
    fn drop(&mut self) {
        let _ = self.command_tx.send(MidiInputCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for MidiService {
    fn drop(&mut self) {
        let _ = self.command_tx.send(MidiCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn note_on(channel: u8, pitch: u8, velocity: u8) -> [u8; 3] {
    [
        0x90 | channel.saturating_sub(1).min(15),
        pitch.min(127),
        velocity.min(127),
    ]
}

pub fn note_off(channel: u8, pitch: u8) -> [u8; 3] {
    [0x80 | channel.saturating_sub(1).min(15), pitch.min(127), 0]
}

pub fn panic_messages() -> Vec<[u8; 3]> {
    let mut messages = Vec::with_capacity(32);
    for channel in 0_u8..16 {
        messages.push([0xB0 | channel, 123, 0]);
        messages.push([0xB0 | channel, 120, 0]);
    }
    messages
}

pub fn decode_note(message: MidiInputMessage) -> Option<NoteInput> {
    if message.len < 3 {
        return None;
    }
    let channel = (message.data[0] & 0x0F) + 1;
    let pitch = message.data[1].min(127);
    match message.data[0] & 0xF0 {
        0x90 if message.data[2] > 0 => Some(NoteInput::On {
            channel,
            pitch,
            velocity: message.data[2].min(127),
        }),
        0x80 | 0x90 => Some(NoteInput::Off { channel, pitch }),
        _ => None,
    }
}

fn worker_loop(command_rx: Receiver<MidiCommand>, event_tx: mpsc::Sender<MidiEvent>) {
    let mut connection: Option<MidiOutputConnection> = None;
    let mut pending = BinaryHeap::<ScheduledMessage>::new();
    let mut order = 0_u64;
    let _ = event_tx.send(MidiEvent::Status(MidiStatus::Disconnected));

    loop {
        send_due(&mut connection, &mut pending, &event_tx);
        let timeout = pending
            .peek()
            .map(|message| {
                message
                    .at
                    .saturating_duration_since(Instant::now())
                    .min(MAX_SCHEDULER_SLEEP)
            })
            .unwrap_or(IDLE_POLL);

        match command_rx.recv_timeout(timeout) {
            Ok(MidiCommand::RefreshPorts) => {
                let ports = output_port_names().unwrap_or_else(|error| {
                    let _ = event_tx.send(MidiEvent::Status(MidiStatus::Error(error.clone())));
                    Vec::new()
                });
                let _ = event_tx.send(MidiEvent::Ports(ports));
            }
            Ok(MidiCommand::Connect(name)) => {
                pending.clear();
                panic_connection(&mut connection);
                connection = None;
                let _ = event_tx.send(MidiEvent::Status(MidiStatus::Connecting(name.clone())));
                match connect_output(&name) {
                    Ok(open_connection) => {
                        connection = Some(open_connection);
                        let _ = event_tx.send(MidiEvent::Status(MidiStatus::Connected(name)));
                    }
                    Err(error) => {
                        let _ = event_tx.send(MidiEvent::Status(MidiStatus::Error(error)));
                    }
                }
            }
            Ok(MidiCommand::Disconnect) => {
                pending.clear();
                panic_connection(&mut connection);
                connection = None;
                let _ = event_tx.send(MidiEvent::Status(MidiStatus::Disconnected));
            }
            Ok(MidiCommand::Schedule { at, data }) => {
                if connection.is_some() {
                    pending.push(ScheduledMessage { at, order, data });
                    order = order.wrapping_add(1);
                }
            }
            Ok(MidiCommand::Panic) => {
                pending.clear();
                panic_connection(&mut connection);
            }
            Ok(MidiCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                pending.clear();
                panic_connection(&mut connection);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn input_worker_loop(
    command_rx: Receiver<MidiInputCommand>,
    event_tx: mpsc::Sender<MidiInputEvent>,
) {
    let mut connection: Option<MidiInputConnection<()>> = None;
    let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Disconnected));
    loop {
        match command_rx.recv() {
            Ok(MidiInputCommand::RefreshPorts) => {
                let ports = input_port_names().unwrap_or_else(|error| {
                    let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Error(error)));
                    Vec::new()
                });
                let _ = event_tx.send(MidiInputEvent::Ports(ports));
            }
            Ok(MidiInputCommand::Connect(name)) => {
                connection = None;
                let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Connecting(name.clone())));
                match connect_input(&name, event_tx.clone()) {
                    Ok(open_connection) => {
                        connection = Some(open_connection);
                        let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Connected(name)));
                    }
                    Err(error) => {
                        let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Error(error)));
                    }
                }
            }
            Ok(MidiInputCommand::Disconnect) => {
                connection = None;
                let _ = event_tx.send(MidiInputEvent::Status(MidiStatus::Disconnected));
            }
            Ok(MidiInputCommand::Shutdown) | Err(_) => {
                drop(connection);
                break;
            }
        }
    }
}

fn output_port_names() -> Result<Vec<String>, String> {
    let output = MidiOutput::new("K.L.A.U.S. Sequencer 58")
        .map_err(|error| format!("MIDI konnte nicht initialisiert werden: {error}"))?;
    let mut names: Vec<String> = output
        .ports()
        .iter()
        .filter_map(|port| output.port_name(port).ok())
        .collect();
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

fn input_port_names() -> Result<Vec<String>, String> {
    let input = MidiInput::new("K.L.A.U.S. Sequencer 58")
        .map_err(|error| format!("MIDI-Eingang konnte nicht initialisiert werden: {error}"))?;
    let mut names: Vec<String> = input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .collect();
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

fn connect_output(name: &str) -> Result<MidiOutputConnection, String> {
    let output = MidiOutput::new("K.L.A.U.S. Sequencer 58")
        .map_err(|error| format!("MIDI konnte nicht initialisiert werden: {error}"))?;
    let ports = output.ports();
    let port = ports
        .iter()
        .find(|port| {
            output
                .port_name(port)
                .is_ok_and(|candidate| candidate == name)
        })
        .ok_or_else(|| format!("MIDI-Ausgang '{name}' wurde nicht gefunden"))?;
    output
        .connect(port, "K.L.A.U.S. MIDI Out")
        .map_err(|error| format!("Verbindung zu '{name}' fehlgeschlagen: {error}"))
}

fn connect_input(
    name: &str,
    event_tx: mpsc::Sender<MidiInputEvent>,
) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new("K.L.A.U.S. Sequencer 58")
        .map_err(|error| format!("MIDI-Eingang konnte nicht initialisiert werden: {error}"))?;
    input.ignore(Ignore::None);
    let ports = input.ports();
    let port = ports
        .iter()
        .find(|port| {
            input
                .port_name(port)
                .is_ok_and(|candidate| candidate == name)
        })
        .ok_or_else(|| format!("MIDI-Eingang '{name}' wurde nicht gefunden"))?;
    input
        .connect(
            port,
            "K.L.A.U.S. MIDI In",
            move |timestamp, bytes, ()| {
                if bytes.is_empty() {
                    return;
                }
                let mut data = [0_u8; 3];
                let len = bytes.len().min(3);
                data[..len].copy_from_slice(&bytes[..len]);
                let _ = event_tx.send(MidiInputEvent::Message(MidiInputMessage {
                    timestamp_micros: timestamp,
                    data,
                    len: len as u8,
                }));
            },
            (),
        )
        .map_err(|error| format!("Verbindung zu MIDI-Eingang '{name}' fehlgeschlagen: {error}"))
}

fn send_due(
    connection: &mut Option<MidiOutputConnection>,
    pending: &mut BinaryHeap<ScheduledMessage>,
    event_tx: &mpsc::Sender<MidiEvent>,
) {
    let now = Instant::now();
    while pending.peek().is_some_and(|message| message.at <= now) {
        let Some(message) = pending.pop() else {
            break;
        };
        if let Some(open_connection) = connection.as_mut()
            && let Err(error) = open_connection.send(&message.data)
        {
            let _ = event_tx.send(MidiEvent::Status(MidiStatus::Error(format!(
                "MIDI-Senden fehlgeschlagen: {error}"
            ))));
        }
    }
}

fn panic_connection(connection: &mut Option<MidiOutputConnection>) {
    if let Some(open_connection) = connection.as_mut() {
        for message in panic_messages() {
            let _ = open_connection.send(&message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MidiInputMessage, NoteInput, decode_note, note_off, note_on, panic_messages};

    #[test]
    fn note_messages_use_one_based_channels_and_clamp_data() {
        assert_eq!(note_on(1, 60, 100), [0x90, 60, 100]);
        assert_eq!(note_on(16, 200, 200), [0x9F, 127, 127]);
        assert_eq!(note_off(3, 64), [0x82, 64, 0]);
    }

    #[test]
    fn panic_covers_all_channels_with_both_safety_controllers() {
        let messages = panic_messages();
        assert_eq!(messages.len(), 32);
        assert_eq!(messages[0], [0xB0, 123, 0]);
        assert_eq!(messages[1], [0xB0, 120, 0]);
        assert_eq!(messages[30], [0xBF, 123, 0]);
        assert_eq!(messages[31], [0xBF, 120, 0]);
    }

    #[test]
    fn note_decoder_treats_zero_velocity_note_on_as_note_off() {
        let note_on_message = MidiInputMessage {
            timestamp_micros: 0,
            data: [0x92, 64, 99],
            len: 3,
        };
        assert_eq!(
            decode_note(note_on_message),
            Some(NoteInput::On {
                channel: 3,
                pitch: 64,
                velocity: 99,
            })
        );
        assert_eq!(
            decode_note(MidiInputMessage {
                data: [0x92, 64, 0],
                ..note_on_message
            }),
            Some(NoteInput::Off {
                channel: 3,
                pitch: 64,
            })
        );
    }
}
