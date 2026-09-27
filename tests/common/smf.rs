//! Standard MIDI File helpers for tests: read a file's notes, dump a file as
//! text (for readable diffs), and build synthetic files.

use midly::num::{u15, u24, u28, u4, u7};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};

/// One sounding note. Note-offs pair with note-ons first-in first-out per
/// (track, channel, key).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SmfNote {
    pub on: u32,
    pub off: u32,
    pub pitch: u8,
    pub vel: u8,
    pub channel: u8,
    pub track: usize,
}

/// Ticks per quarter note (0 for SMPTE timing).
pub fn ppq(bytes: &[u8]) -> u16 {
    match Smf::parse(bytes).expect("valid SMF").header.timing {
        Timing::Metrical(t) => t.as_int(),
        Timing::Timecode(..) => 0,
    }
}

/// Every note of the file, sorted by (on, pitch, track).
pub fn notes(bytes: &[u8]) -> Vec<SmfNote> {
    let smf = Smf::parse(bytes).expect("valid SMF");
    let mut out = Vec::new();
    for (track, events) in smf.tracks.iter().enumerate() {
        let mut open: std::collections::HashMap<(u8, u8), Vec<(u32, u8)>> = Default::default();
        let mut t = 0u32;
        for ev in events {
            t += ev.delta.as_int();
            let TrackEventKind::Midi { channel, message } = ev.kind else {
                continue;
            };
            let ch = channel.as_int();
            match message {
                MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                    open.entry((ch, key.as_int()))
                        .or_default()
                        .push((t, vel.as_int()));
                }
                MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                    let q = open.entry((ch, key.as_int())).or_default();
                    if !q.is_empty() {
                        let (on, vel) = q.remove(0);
                        out.push(SmfNote {
                            on,
                            off: t,
                            pitch: key.as_int(),
                            vel,
                            channel: ch,
                            track,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    out.sort_by_key(|n| (n.on, n.pitch, n.track, n.off));
    out
}

/// The file as text, one event per line: `track tick event…`. Note-ons and
/// note-offs are listed as paired notes (`note on-off pitch vel ch`).
pub fn dump(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let smf = Smf::parse(bytes).expect("valid SMF");
    let mut s = String::new();
    let _ = writeln!(
        s,
        "format {:?} timing {:?} tracks {}",
        smf.header.format,
        smf.header.timing,
        smf.tracks.len()
    );
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    for (track, events) in smf.tracks.iter().enumerate() {
        let mut t = 0u32;
        for ev in events {
            t += ev.delta.as_int();
            let line = match ev.kind {
                TrackEventKind::Meta(m) => match m {
                    MetaMessage::TrackName(b) => format!("name {:?}", text(b)),
                    MetaMessage::InstrumentName(b) => format!("instrument {:?}", text(b)),
                    MetaMessage::Text(b) => format!("text {:?}", text(b)),
                    MetaMessage::Lyric(b) => format!("lyric {:?}", text(b)),
                    MetaMessage::Marker(b) => format!("marker {:?}", text(b)),
                    MetaMessage::Tempo(us) => format!("tempo {}", us.as_int()),
                    MetaMessage::TimeSignature(n, d, ..) => format!("time {n}/{}", 1u32 << d),
                    MetaMessage::KeySignature(sf, minor) => format!("key {sf} {}", u8::from(minor)),
                    MetaMessage::EndOfTrack => continue,
                    other => format!("meta {other:?}"),
                },
                TrackEventKind::Midi { channel, message } => match message {
                    MidiMessage::NoteOn { .. } | MidiMessage::NoteOff { .. } => continue,
                    MidiMessage::Controller { controller, value } => {
                        format!("cc ch{} {} {}", channel, controller, value)
                    }
                    MidiMessage::ProgramChange { program } => {
                        format!("program ch{channel} {program}")
                    }
                    other => format!("midi ch{channel} {other:?}"),
                },
                other => format!("{other:?}"),
            };
            let _ = writeln!(s, "{track} {t} {line}");
        }
    }
    for n in notes(bytes) {
        let _ = writeln!(
            s,
            "{} {} note {}-{} {} {} ch{}",
            n.track, n.on, n.on, n.off, n.pitch, n.vel, n.channel
        );
    }
    s
}

/// A meta or controller event in a synthetic file.
#[derive(Clone, Debug)]
pub enum Ev {
    Time(u8, u8),
    Key(i8, bool),
    /// Microseconds per quarter note.
    Tempo(u32),
    Text(String),
    Lyric(String),
    /// Controller number and value, on the track's channel.
    Cc(u8, u8),
    Program(u8),
}

/// A synthetic track: notes are `(on, off, pitch, velocity)` in ticks.
#[derive(Clone, Debug, Default)]
pub struct Track {
    pub name: String,
    pub channel: u8,
    pub notes: Vec<(u32, u32, u8, u8)>,
    pub events: Vec<(u32, Ev)>,
}

impl Track {
    pub fn new(name: &str, channel: u8, notes: &[(u32, u32, u8, u8)]) -> Track {
        Track {
            name: name.to_string(),
            channel,
            notes: notes.to_vec(),
            events: Vec::new(),
        }
    }
}

/// Build a format-1 file: track 0 holds `conductor`, then one track per
/// `tracks` entry. At equal ticks note-offs come before note-ons.
pub fn build(ppq: u16, conductor: &[(u32, Ev)], tracks: &[Track]) -> Vec<u8> {
    let mut smf = Smf::new(Header::new(
        Format::Parallel,
        Timing::Metrical(u15::new(ppq)),
    ));
    let mut all = vec![(String::new(), 0u8, conductor.to_vec(), Vec::new())];
    for t in tracks {
        all.push((t.name.clone(), t.channel, t.events.clone(), t.notes.clone()));
    }
    for (name, ch, events, notes) in &all {
        // (tick, order, kind): order puts metas first, then offs, then ons.
        let mut evs: Vec<(u32, u8, TrackEventKind)> = Vec::new();
        if !name.is_empty() {
            evs.push((
                0,
                0,
                TrackEventKind::Meta(MetaMessage::TrackName(name.as_bytes())),
            ));
        }
        let channel = u4::new(*ch);
        for (tick, e) in events {
            let kind = match e {
                Ev::Time(n, d) => TrackEventKind::Meta(MetaMessage::TimeSignature(
                    *n,
                    d.trailing_zeros() as u8,
                    24,
                    8,
                )),
                Ev::Key(sf, minor) => TrackEventKind::Meta(MetaMessage::KeySignature(*sf, *minor)),
                Ev::Tempo(us) => TrackEventKind::Meta(MetaMessage::Tempo(u24::new(*us))),
                Ev::Text(s) => TrackEventKind::Meta(MetaMessage::Text(s.as_bytes())),
                Ev::Lyric(s) => TrackEventKind::Meta(MetaMessage::Lyric(s.as_bytes())),
                Ev::Cc(c, v) => TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::Controller {
                        controller: u7::new(*c),
                        value: u7::new(*v),
                    },
                },
                Ev::Program(p) => TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::ProgramChange {
                        program: u7::new(*p),
                    },
                },
            };
            evs.push((*tick, 0, kind));
        }
        for &(on, off, pitch, vel) in notes {
            evs.push((
                on,
                2,
                TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn {
                        key: u7::new(pitch),
                        vel: u7::new(vel),
                    },
                },
            ));
            evs.push((
                off,
                1,
                TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOff {
                        key: u7::new(pitch),
                        vel: u7::new(0),
                    },
                },
            ));
        }
        evs.sort_by_key(|(t, o, _)| (*t, *o));
        let mut track = Vec::new();
        let mut last = 0;
        for (t, _, kind) in evs {
            track.push(TrackEvent {
                delta: u28::new(t - last),
                kind,
            });
            last = t;
        }
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        smf.tracks.push(track);
    }
    let mut out = Vec::new();
    smf.write_std(&mut out).expect("write SMF");
    out
}
