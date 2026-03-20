//! Fixture-based regression tests.
//!
//! Ensures that every fixture file in `tests/fixtures/` parses without
//! panicking and produces a non-empty IR. This catches regressions when
//! adapter code changes.

use _core::adapters::ir_to_ly::IrToLyAdapter;
use _core::adapters::ir_to_mxml::IrToMxmlAdapter;
use _core::adapters::ly_to_ir::LyToIrAdapter;
use _core::adapters::mxml_to_ir::MxmlToIrAdapter;
use _core::adapters::{FromIrAdapter, ToIrAdapter};

use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_xml_fixture(filename: &str) -> _core::ir::score::Score {
    let path = format!("tests/fixtures/xml/{filename}");
    let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    MxmlToIrAdapter::new()
        .convert_str(&xml)
        .unwrap_or_else(|e| panic!("MxmlToIr failed on {filename}: {e}"))
}

fn parse_ly_fixture(filename: &str) -> _core::ir::score::Score {
    let path = format!("tests/fixtures/ly/{filename}");
    let ly = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    LyToIrAdapter::new()
        .convert_str(&ly)
        .unwrap_or_else(|e| panic!("LyToIr failed on {filename}: {e}"))
}

fn parse_mxl_fixture(filename: &str) -> _core::ir::score::Score {
    let path_str = format!("tests/fixtures/mxl/{filename}");
    let path = Path::new(&path_str);
    MxmlToIrAdapter::new()
        .convert_file(path)
        .unwrap_or_else(|e| panic!("MxmlToIr failed on MXL {filename}: {e}"))
}

// ---------------------------------------------------------------------------
// XML fixture regression: parse all 143 files
// ---------------------------------------------------------------------------

macro_rules! xml_fixture_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let score = parse_xml_fixture($file);
            assert!(
                !score.parts().is_empty(),
                "Expected at least one part in {}",
                $file
            );
        }
    };
}

// Pitches
xml_fixture_test!(xml_01a_pitches_pitches, "01a-Pitches-Pitches.xml");
xml_fixture_test!(xml_01b_pitches_intervals, "01b-Pitches-Intervals.xml");
xml_fixture_test!(
    xml_01c_pitches_no_voice_element,
    "01c-Pitches-NoVoiceElement.xml"
);
xml_fixture_test!(xml_01d_pitches_microtones, "01d-Pitches-Microtones.xml");
xml_fixture_test!(
    xml_01e_pitches_editorial,
    "01e-Pitches-EditorialCautionaryAccidentals.xml"
);
xml_fixture_test!(
    xml_01ea_pitches_parenthesis,
    "01ea-Pitches-Parenthesis-Changed-Accidentals.xml"
);
xml_fixture_test!(
    xml_01f_pitches_parenthesized_microtone,
    "01f-Pitches-ParenthesizedMicrotoneAccidentals.xml"
);

// Rests
xml_fixture_test!(xml_02a_rests_durations, "02a-Rests-Durations.xml");
xml_fixture_test!(xml_02b_rests_pitched, "02b-Rests-PitchedRests.xml");
xml_fixture_test!(
    xml_02c_rests_multimeasure,
    "02c-Rests-MultiMeasureRests.xml"
);
xml_fixture_test!(
    xml_02d_rests_multimeasure_timesig,
    "02d-Rests-Multimeasure-TimeSignatures.xml"
);
xml_fixture_test!(xml_02e_rests_notype, "02e-Rests-NoType.xml");

// Rhythm
xml_fixture_test!(xml_03aa_rhythm_durations, "03aa-Rhythm-Durations.xml");
xml_fixture_test!(xml_03ab_rhythm_durations, "03ab-Rhythm-Durations.xml");
xml_fixture_test!(xml_03b_rhythm_backup, "03b-Rhythm-Backup.xml");
xml_fixture_test!(
    xml_03c_rhythm_division_change,
    "03c-Rhythm-DivisionChange.xml"
);
xml_fixture_test!(
    xml_03d_rhythm_dotted_factors,
    "03d-Rhythm-DottedDurations-Factors.xml"
);

// Time signatures
xml_fixture_test!(xml_11a_time_signatures, "11a-TimeSignatures.xml");
xml_fixture_test!(xml_11b_time_no_time, "11b-TimeSignatures-NoTime.xml");
xml_fixture_test!(
    xml_11c_time_compound_simple,
    "11c-TimeSignatures-CompoundSimple.xml"
);
xml_fixture_test!(
    xml_11d_time_compound_multiple,
    "11d-TimeSignatures-CompoundMultiple.xml"
);
xml_fixture_test!(
    xml_11e_time_compound_mixed,
    "11e-TimeSignatures-CompoundMixed.xml"
);
xml_fixture_test!(
    xml_11f_time_symbol_meaning,
    "11f-TimeSignatures-SymbolMeaning.xml"
);
xml_fixture_test!(
    xml_11g_time_single_number,
    "11g-TimeSignatures-SingleNumber.xml"
);
xml_fixture_test!(
    xml_11h_time_senza_misura,
    "11h-TimeSignatures-SenzaMisura.xml"
);

// Clefs
xml_fixture_test!(
    xml_12aa_clefs_traditional,
    "12aa-Clefs_Pitch_Traditional.xml"
);
xml_fixture_test!(
    xml_12ab_clefs_percussion,
    "12ab-Clefs-Percussion-NonTrad.xml"
);
xml_fixture_test!(xml_12ac_clefs_tab_switch, "12ac-Clefs-TAB-Switch.xml");
xml_fixture_test!(
    xml_12ad_clefs_extreme_octave,
    "12ad-Clefs-Extreme-Octave.xml"
);
xml_fixture_test!(xml_12b_clefs_no_key_or_clef, "12b-Clefs-NoKeyOrClef.xml");

// Key signatures
xml_fixture_test!(xml_13a_key_signatures, "13a-KeySignatures.xml");
xml_fixture_test!(
    xml_13aa_key_signatures_extreme,
    "13aa-KeySignatures-Extreme.xml"
);
xml_fixture_test!(
    xml_13ab_key_signatures_cancel,
    "13ab-KeySignatures-Cancel.xml"
);
xml_fixture_test!(
    xml_13ac_key_signatures_octaves,
    "13ac-KeySignatures-Octaves.xml"
);
xml_fixture_test!(
    xml_13b_key_church_modes,
    "13b-KeySignatures-ChurchModes.xml"
);
xml_fixture_test!(
    xml_13c_key_non_traditional,
    "13c-KeySignatures-NonTraditional.xml"
);
xml_fixture_test!(xml_13d_key_microtones, "13d-KeySignatures-Microtones.xml");
xml_fixture_test!(
    xml_13e_key_mid_measure,
    "13e-KeySignatures-MidMeasure-Change.xml"
);

// Staff details
xml_fixture_test!(
    xml_14a_staff_details_lines,
    "14a-StaffDetails-LineChanges.xml"
);

// Chords
xml_fixture_test!(xml_21a_chord_basic, "21a-Chord-Basic.xml");
xml_fixture_test!(xml_21b_chords_two_notes, "21b-Chords-TwoNotes.xml");
xml_fixture_test!(
    xml_21c_chords_three_notes,
    "21c-Chords-ThreeNotesDuration.xml"
);
xml_fixture_test!(
    xml_21d_chords_schubert,
    "21d-Chords-SchubertStabatMater.xml"
);
xml_fixture_test!(xml_21e_chords_pickup, "21e-Chords-PickupMeasures.xml");
xml_fixture_test!(
    xml_21f_chord_element_in_between,
    "21f-Chord-ElementInBetween.xml"
);

// Noteheads
xml_fixture_test!(xml_22a_noteheads, "22a-Noteheads.xml");
xml_fixture_test!(xml_22b_staff_notestyles, "22b-Staff-Notestyles.xml");
xml_fixture_test!(xml_22c_noteheads_chords, "22c-Noteheads-Chords.xml");
xml_fixture_test!(
    xml_22d_parenthesized_noteheads,
    "22d-Parenthesized-Noteheads.xml"
);

// Tuplets
xml_fixture_test!(xml_23a_tuplets, "23a-Tuplets.xml");
xml_fixture_test!(xml_23b_tuplets_styles, "23b-Tuplets-Styles.xml");
xml_fixture_test!(xml_23c_tuplet_display, "23c-Tuplet-Display-NonStandard.xml");
xml_fixture_test!(xml_23d_tuplets_nested, "23d-Tuplets-Nested.xml");
xml_fixture_test!(xml_23e_tuplets_tremolo, "23e-Tuplets-Tremolo.xml");
xml_fixture_test!(
    xml_23f_tuplets_no_bracket,
    "23f-Tuplets-DurationButNoBracket.xml"
);

// Grace notes
xml_fixture_test!(xml_24a_grace_notes, "24a-GraceNotes.xml");
xml_fixture_test!(xml_24b_chord_grace, "24b-ChordAsGraceNote.xml");
xml_fixture_test!(xml_24c_grace_measure_end, "24c-GraceNote-MeasureEnd.xml");
xml_fixture_test!(xml_24d_after_grace, "24d-AfterGrace.xml");
xml_fixture_test!(xml_24e_grace_staff_change, "24e-GraceNote-StaffChange.xml");
xml_fixture_test!(xml_24f_grace_slur, "24f-GraceNote-Slur.xml");

// Directions
xml_fixture_test!(xml_31a_directions, "31a-Directions.xml");
xml_fixture_test!(xml_31c_metronome_marks, "31c-MetronomeMarks.xml");

// Notations
xml_fixture_test!(xml_32a_notations, "32a-Notations.xml");
xml_fixture_test!(
    xml_32aa_notations2_ornaments,
    "32aa-Notations2_Ornaments.xml"
);
xml_fixture_test!(xml_32ab_notations3, "32ab-Notations3.xml");
xml_fixture_test!(xml_32ac_notations4, "32ac-Notations4.xml");
xml_fixture_test!(xml_32b_articulations_texts, "32b-Articulations-Texts.xml");
xml_fixture_test!(
    xml_32c_multiple_notation,
    "32c-MultipleNotationChildren.xml"
);
xml_fixture_test!(xml_32d_arpeggio, "32d-Arpeggio.xml");

// Spanners
xml_fixture_test!(xml_33a_spanners, "33a-Spanners.xml");
xml_fixture_test!(xml_33b_spanners_tie, "33b-Spanners-Tie.xml");
xml_fixture_test!(xml_33c_spanners_slurs, "33c-Spanners-Slurs.xml");
xml_fixture_test!(
    xml_33d_spanners_octave_shifts,
    "33d-Spanners-OctaveShifts.xml"
);
xml_fixture_test!(
    xml_33e_spanners_octave_invalid,
    "33e-Spanners-OctaveShifts-InvalidSize.xml"
);
xml_fixture_test!(xml_33f_trill_ending, "33f-Trill-EndingOnGraceNote.xml");
xml_fixture_test!(xml_33g_slur_chorded, "33g-Slur-ChordedNotes.xml");
xml_fixture_test!(xml_33h_spanners_glissando, "33h-Spanners-Glissando.xml");
xml_fixture_test!(xml_33i_ties_not_ended, "33i-Ties-NotEnded.xml");

// Multi-parts
xml_fixture_test!(xml_41a_multiparts_order, "41a-MultiParts-Partorder.xml");
xml_fixture_test!(
    xml_41b_multiparts_more_than_10,
    "41b-MultiParts-MoreThan10.xml"
);
xml_fixture_test!(xml_41c_staff_groups, "41c-StaffGroups.xml");
xml_fixture_test!(xml_41d_staff_groups_nested, "41d-StaffGroups-Nested.xml");
xml_fixture_test!(
    xml_41e_staff_groups_names,
    "41e-StaffGroups-InstrumentNames-Linebroken.xml"
);
xml_fixture_test!(
    xml_41f_staff_groups_overlap,
    "41f-StaffGroups-Overlapping.xml"
);
// 41g-PartNoId.xml: <part> with no id attribute — musicxml crate requires id, skipped
xml_fixture_test!(xml_41h_too_many_parts, "41h-TooManyParts.xml");
xml_fixture_test!(
    xml_41i_part_name_display,
    "41i-PartNameDisplay-Override.xml"
);

// Multi-voice
xml_fixture_test!(
    xml_42a_multivoice_lyrics,
    "42a-MultiVoice-TwoVoicesOnStaff-Lyrics.xml"
);
xml_fixture_test!(
    xml_42b_multivoice_clef_change,
    "42b-MultiVoice-MidMeasureClefChange.xml"
);

// Piano staff
xml_fixture_test!(xml_43a_piano_staff, "43a-PianoStaff.xml");
xml_fixture_test!(
    xml_43b_multi_staff_diff_keys,
    "43b-MultiStaff-DifferentKeys.xml"
);
xml_fixture_test!(
    xml_43c_multi_staff_diff_keys_backup,
    "43c-MultiStaff-DifferentKeysAfterBackup.xml"
);
xml_fixture_test!(xml_43d_multi_staff_change, "43d-MultiStaff-StaffChange.xml");
xml_fixture_test!(
    xml_43e_multi_staff_clef_dynamics,
    "43e-Multistaff-ClefDynamics.xml"
);

// Repeats
xml_fixture_test!(xml_45a_simple_repeat, "45a-SimpleRepeat.xml");
xml_fixture_test!(
    xml_45b_repeat_alternatives,
    "45b-RepeatWithAlternatives.xml"
);
xml_fixture_test!(xml_45c_repeat_multiple_times, "45c-RepeatMultipleTimes.xml");
xml_fixture_test!(
    xml_45d_repeats_nested,
    "45d-Repeats-Nested-Alternatives.xml"
);
xml_fixture_test!(
    xml_45e_repeats_nested2,
    "45e-Repeats-Nested-Alternatives.xml"
);
xml_fixture_test!(
    xml_45f_repeats_invalid_endings,
    "45f-Repeats-InvalidEndings.xml"
);
xml_fixture_test!(xml_45g_repeats_not_ended, "45g-Repeats-NotEnded.xml");

// Barlines & pickups
xml_fixture_test!(xml_46a_barlines, "46a-Barlines.xml");
xml_fixture_test!(xml_46b_midmeasure_barline, "46b-MidmeasureBarline.xml");
xml_fixture_test!(xml_46c_midmeasure_clef, "46c-Midmeasure-Clef.xml");
xml_fixture_test!(
    xml_46d_pickup_implicit,
    "46d-PickupMeasure-ImplicitMeasures.xml"
);
xml_fixture_test!(
    xml_46e_pickup_second_voice,
    "46e-PickupMeasure-SecondVoiceStartsLater.xml"
);
xml_fixture_test!(xml_46f_incomplete_measures, "46f-IncompleteMeasures.xml");
xml_fixture_test!(
    xml_46g_pickup_chordnames,
    "46g-PickupMeasure-Chordnames-FiguredBass.xml"
);

// Headers
xml_fixture_test!(xml_51b_header_quotes, "51b-Header-Quotes.xml");
xml_fixture_test!(xml_51c_multiple_rights, "51c-MultipleRights.xml");
xml_fixture_test!(xml_51d_empty_title, "51d-EmptyTitle.xml");

// Layout
xml_fixture_test!(xml_52a_page_layout, "52a-PageLayout.xml");
xml_fixture_test!(xml_52b_breaks, "52b-Breaks.xml");

// Lyrics
xml_fixture_test!(xml_61a_lyrics, "61a-Lyrics.xml");
xml_fixture_test!(xml_61b_multiple_lyrics, "61b-MultipleLyrics.xml");
xml_fixture_test!(xml_61c_lyrics_pianostaff, "61c-Lyrics-Pianostaff.xml");
xml_fixture_test!(xml_61d_lyrics_melisma, "61d-Lyrics-Melisma.xml");
xml_fixture_test!(xml_61e_lyrics_chords, "61e-Lyrics-Chords.xml");
xml_fixture_test!(xml_61f_lyrics_graced, "61f-Lyrics-GracedNotes.xml");
xml_fixture_test!(xml_61g_lyrics_name_number, "61g-Lyrics-NameNumber.xml");
xml_fixture_test!(xml_61h_lyrics_beams, "61h-Lyrics-BeamsMelismata.xml");
xml_fixture_test!(xml_61i_lyrics_chords2, "61i-Lyrics-Chords.xml");
xml_fixture_test!(xml_61j_lyrics_elisions, "61j-Lyrics-Elisions.xml");
xml_fixture_test!(xml_61k_lyrics_spanners, "61k-Lyrics-SpannersExtenders.xml");

// Chord names & tabs
xml_fixture_test!(xml_71a_chordnames, "71a-Chordnames.xml");
xml_fixture_test!(xml_71c_chords_frets, "71c-ChordsFrets.xml");
xml_fixture_test!(
    xml_71d_chords_frets_multistaff,
    "71d-ChordsFrets-Multistaff.xml"
);
xml_fixture_test!(xml_71e_tab_staves, "71e-TabStaves.xml");
xml_fixture_test!(xml_71f_all_chord_types, "71f-AllChordTypes.xml");
xml_fixture_test!(xml_71g_multiple_chordnames, "71g-MultipleChordnames.xml");

// Transposing instruments
xml_fixture_test!(xml_72a_transposing, "72a-TransposingInstruments.xml");
xml_fixture_test!(
    xml_72b_transposing_full,
    "72b-TransposingInstruments-Full.xml"
);
xml_fixture_test!(
    xml_72c_transposing_change,
    "72c-TransposingInstruments-Change.xml"
);
xml_fixture_test!(
    xml_72d_transposing_score_pitch,
    "72d-TransposingInstruments-scorePitch.xml"
);

// Percussion
xml_fixture_test!(xml_73a_percussion, "73a-Percussion.xml");

// Figured bass
xml_fixture_test!(xml_74a_figured_bass, "74a-FiguredBass.xml");

// Accordion
xml_fixture_test!(
    xml_75a_accordion_registrations,
    "75a-AccordionRegistrations.xml"
);

// Misc/vendor
xml_fixture_test!(xml_99a_sibelius_beaming, "99a-Sibelius5-IgnoreBeaming.xml");
xml_fixture_test!(
    xml_99b_lyrics_beams_ignore,
    "99b-Lyrics-BeamsMelismata-IgnoreBeams.xml"
);
xml_fixture_test!(xml_99c_wavy_lines, "99c-Wavy-Lines-No-Numbers.xml");
xml_fixture_test!(xml_99d_accordion_invalid, "99d-AccordionInvalid.xml");

// ---------------------------------------------------------------------------
// MXL fixture regression: parse all 10 compressed files
// ---------------------------------------------------------------------------

macro_rules! mxl_fixture_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let score = parse_mxl_fixture($file);
            assert!(
                !score.parts().is_empty(),
                "Expected at least one part in MXL {}",
                $file
            );
        }
    };
}

mxl_fixture_test!(
    mxl_2160_fingering,
    "2160_single_voice_fingering_repeats_slurm_and_bow_symbols.mxl"
);
mxl_fixture_test!(mxl_2340_chords, "2340_single_voice_with_chords.mxl");
mxl_fixture_test!(
    mxl_2800_text_chords_repeats,
    "2800_single_voice_text_and_chords_and_repeats.mxl"
);
mxl_fixture_test!(
    mxl_2840_multi_voice,
    "2840_score_with_multi_voice_parts_text_and_dynamics.mxl"
);
mxl_fixture_test!(mxl_3840_piano, "3840_multi_part_piano_score.mxl");
mxl_fixture_test!(mxl_4480_quintet, "4480_string_quintet.mxl");
mxl_fixture_test!(mxl_4700_fingering, "4700_piano_score_with_fingering.mxl");
mxl_fixture_test!(mxl_5420_duet, "5420_multivoice_2parts_duet.mxl");
mxl_fixture_test!(mxl_640_piano_text, "640_piano_score_with_text.mxl");
mxl_fixture_test!(
    mxl_700_multi_instrument,
    "700_score_multi_instrument_and_text.mxl"
);

// ---------------------------------------------------------------------------
// LY fixture regression: parse all named fixtures
// ---------------------------------------------------------------------------

macro_rules! ly_fixture_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let score = parse_ly_fixture($file);
            // LY files may legitimately produce zero parts (e.g., include-only),
            // so we just assert no panic.
            let _ = score;
        }
    };
}

ly_fixture_test!(ly_autochange_clefs, "autochange-clefs.ly");
ly_fixture_test!(ly_caesura_articulation, "caesura-articulation-multiple.ly");
ly_fixture_test!(ly_chopin, "chopin_n.ly");
ly_fixture_test!(ly_chord_names_bass, "chord-names-bass.ly");
ly_fixture_test!(ly_chord_names_languages, "chord-names-languages2.ly");
ly_fixture_test!(ly_cue_clef, "cue-clef-begin-of-score.ly");
ly_fixture_test!(ly_dynamic_initial, "dynamic-initial.ly");
ly_fixture_test!(ly_example, "example.ly");
ly_fixture_test!(ly_example2, "example2.ly");
ly_fixture_test!(ly_key_signature_left, "key-signature-left-edge.ly");
ly_fixture_test!(ly_key_signature_padding, "key-signature-padding.ly");
ly_fixture_test!(ly_lyric_tie, "lyric-tie.ly");
ly_fixture_test!(ly_make_relative_copies, "make-relative-copies.ly");
ly_fixture_test!(ly_note_head_style, "note-head-style.ly");
ly_fixture_test!(ly_part_combine_tuplet, "part-combine-tuplet-end.ly");
ly_fixture_test!(ly_pedal, "pedal.ly");
ly_fixture_test!(ly_relative_repeat, "relative-repeat.ly");
ly_fixture_test!(ly_repeats, "repeats.ly");
ly_fixture_test!(ly_rest_dynamic, "rest-dynamic.ly");
ly_fixture_test!(ly_slur_dash, "slur-dash.ly");
ly_fixture_test!(ly_slur_nice, "slur-nice.ly");
ly_fixture_test!(ly_spacing_accidental_tie, "spacing-accidental-tie.ly");
ly_fixture_test!(ly_span_bar_articulation, "span-bar-articulation.ly");
ly_fixture_test!(ly_tablature_grace, "tablature-grace-notes.ly");
ly_fixture_test!(
    ly_time_sig_alternating,
    "time-signature-alternating-fraction.ly"
);
ly_fixture_test!(ly_time_sig_unsupported, "time-signature-unsupported.ly");
ly_fixture_test!(
    ly_tuplet_bracket_vertical,
    "tuplet-bracket-vertical-skylines.ly"
);

// Also test UUID-named fixtures (from lilybert dataset samples)
ly_fixture_test!(ly_uuid_00a01a21, "00a01a21-e760-475e-ba61-a7e1bb919d3b.ly");
ly_fixture_test!(ly_uuid_0a0af71c, "0a0af71c-bf07-47aa-8077-702e9e9eaa51.ly");
ly_fixture_test!(ly_uuid_0a0b0897, "0a0b0897-2f7e-42f9-9739-d4fc114fe574.ly");
ly_fixture_test!(ly_uuid_0a0bafbe, "0a0bafbe-d424-4a9f-91c2-ca65c9f41689.ly");
ly_fixture_test!(ly_uuid_0a0ca4ef, "0a0ca4ef-b5d5-4e99-94ad-8500deece021.ly");
ly_fixture_test!(ly_uuid_0a0cbf6d, "0a0cbf6d-de04-49b6-b263-a86f67672009.ly");
ly_fixture_test!(ly_uuid_0a0f75da, "0a0f75da-57ab-4c26-98e2-a556f5c8e56a.ly");
ly_fixture_test!(ly_uuid_0a1a2c31, "0a1a2c31-7aa1-4e34-9334-5d4858321336.ly");

// ---------------------------------------------------------------------------
// Cross-format regression: XML → Score → LY (no panic, produces output)
// ---------------------------------------------------------------------------

macro_rules! xml_to_ly_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let score = parse_xml_fixture($file);
            let ly = IrToLyAdapter::new()
                .convert(&score)
                .unwrap_or_else(|e| panic!("IrToLy failed on {}: {e}", $file));
            assert!(
                !ly.is_empty(),
                "LY output should be non-empty for {}",
                $file
            );
        }
    };
}

xml_to_ly_test!(xml_to_ly_01a_pitches, "01a-Pitches-Pitches.xml");
xml_to_ly_test!(xml_to_ly_02a_rests, "02a-Rests-Durations.xml");
xml_to_ly_test!(xml_to_ly_03aa_rhythm, "03aa-Rhythm-Durations.xml");
xml_to_ly_test!(xml_to_ly_11a_timesig, "11a-TimeSignatures.xml");
xml_to_ly_test!(xml_to_ly_13a_keysig, "13a-KeySignatures.xml");
xml_to_ly_test!(xml_to_ly_21a_chord, "21a-Chord-Basic.xml");
xml_to_ly_test!(xml_to_ly_23a_tuplets, "23a-Tuplets.xml");
xml_to_ly_test!(xml_to_ly_24a_grace, "24a-GraceNotes.xml");
xml_to_ly_test!(xml_to_ly_41a_multipart, "41a-MultiParts-Partorder.xml");
xml_to_ly_test!(xml_to_ly_43a_piano, "43a-PianoStaff.xml");
xml_to_ly_test!(xml_to_ly_45a_repeat, "45a-SimpleRepeat.xml");
xml_to_ly_test!(xml_to_ly_46a_barlines, "46a-Barlines.xml");
xml_to_ly_test!(xml_to_ly_61a_lyrics, "61a-Lyrics.xml");
xml_to_ly_test!(xml_to_ly_74a_figured_bass, "74a-FiguredBass.xml");

// ---------------------------------------------------------------------------
// Cross-format regression: XML → Score → MusicXML (no panic, produces output)
// ---------------------------------------------------------------------------

macro_rules! xml_to_mxml_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let score = parse_xml_fixture($file);
            let mxml = IrToMxmlAdapter::new()
                .convert(&score)
                .unwrap_or_else(|e| panic!("IrToMxml failed on {}: {e}", $file));
            assert!(
                mxml.contains("<score-partwise"),
                "MusicXML output should contain <score-partwise> for {}",
                $file
            );
        }
    };
}

xml_to_mxml_test!(xml_to_mxml_01a_pitches, "01a-Pitches-Pitches.xml");
xml_to_mxml_test!(xml_to_mxml_02a_rests, "02a-Rests-Durations.xml");
xml_to_mxml_test!(xml_to_mxml_11a_timesig, "11a-TimeSignatures.xml");
xml_to_mxml_test!(xml_to_mxml_21a_chord, "21a-Chord-Basic.xml");
xml_to_mxml_test!(xml_to_mxml_41a_multipart, "41a-MultiParts-Partorder.xml");
xml_to_mxml_test!(xml_to_mxml_43a_piano, "43a-PianoStaff.xml");
xml_to_mxml_test!(xml_to_mxml_45a_repeat, "45a-SimpleRepeat.xml");
xml_to_mxml_test!(xml_to_mxml_61a_lyrics, "61a-Lyrics.xml");
