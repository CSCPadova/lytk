"""The 2026-10 review's findings (Epic M), one test per confirmed bug.

Each test asserts the right behaviour; each was a strict expected failure
until the Epic M task that fixed it.
"""

from __future__ import annotations

import re

import lytk


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def ly(text: str) -> lytk.Score:
    return lytk.from_lilypond_string(text)


def syllables(score: lytk.Score) -> list[list[str]]:
    """Per note (chord notes included), the texts of its syllables."""
    return [n.lyrics for p in score.iter_parts() for n in p.notes]


def ir_notes(score: lytk.Score):
    """Every note dict of the raw IR, in document order."""
    out = []

    def walk(o):
        if isinstance(o, dict):
            if "measures" in o:
                for m in o["measures"]:
                    for v in m["voices"]:
                        for e in v["elements"]:
                            if "Note" in e:
                                out.append(e["Note"])
                            elif "Chord" in e:
                                out.extend(e["Chord"]["notes"])
                return
            for v in o.values():
                walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)

    walk(score.to_dict())
    return out


def ir_measures(score: lytk.Score):
    out = []

    def walk(o):
        if isinstance(o, dict):
            if "measures" in o:
                out.extend(o["measures"])
                return
            for v in o.values():
                walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)

    walk(score.to_dict())
    return out


def stems(xml: str) -> list[str]:
    return re.findall(r"<stem>(\w+)</stem>", xml)


def xml_part(measures: str, attrs: str = "") -> str:
    return (
        '<?xml version="1.0" encoding="UTF-8"?>\n<score-partwise version="4.0">'
        '<part-list><score-part id="P1"><part-name>x</part-name></score-part></part-list>'
        '<part id="P1"><measure number="1"><attributes><divisions>1</divisions>'
        f"{attrs}<time><beats>4</beats><beat-type>4</beat-type></time></attributes>"
        f"{measures}</measure></part></score-partwise>"
    )


def xml_note(step: str, extra: str = "", octave: int = 4, dur: int = 1) -> str:
    kind = {1: "quarter", 2: "half", 4: "whole"}[dur]
    return (
        f"<note><pitch><step>{step}</step><octave>{octave}</octave></pitch>"
        f"<duration>{dur}</duration><voice>1</voice><type>{kind}</type>{extra}</note>"
    )


def lyric(text: str, number: str = "1", syllabic: str = "single") -> str:
    return f'<lyric number="{number}"><syllabic>{syllabic}</syllabic><text>{text}</text></lyric>'


# ---------------------------------------------------------------------------
# F1 — lyrics
# ---------------------------------------------------------------------------


class TestLyricsReading:
    def test_slur_is_a_melisma(self):
        s = ly(r"\relative c' { c4 d( e) f } \addlyrics { a b c d }")
        assert syllables(s) == [["a"], ["b"], [], ["c"]]

    def test_tie_is_a_melisma(self):
        s = ly(r"\relative c' { c4 d~ d f } \addlyrics { a b c d }")
        assert syllables(s) == [["a"], ["b"], [], ["c"]]

    def test_extender_consumes_no_note(self):
        s = ly(r"\relative c' { c4 d e f } \addlyrics { a __ b c d }")
        assert syllables(s) == [["a"], ["b"], ["c"], ["d"]]
        assert ir_notes(s)[0]["lyrics"][0]["extend"] is True

    def test_lyric_tie_is_one_elided_syllable(self):
        s = ly(r"{ c'4 d' e' f' } \addlyrics { a~b c d e }")
        assert syllables(s)[0] == ["a‿b"]
        assert ir_notes(s)[0]["lyrics"][0]["elision"] is True

    def test_underscore_in_a_word_is_a_space(self):
        s = ly(r"{ c'4 d' e' f' } \addlyrics { a_b c d e }")
        assert syllables(s)[0] == ["a b"]

    def test_punctuation_stays_in_the_word(self):
        s = ly(r"{ c'4 d' e' f' } \addlyrics { don't stop, now! go. }")
        assert syllables(s) == [["don't"], ["stop,"], ["now!"], ["go."]]

    def test_two_addlyrics_are_two_verses(self):
        s = ly(r"{ c'4 d' e' f' } \addlyrics { a b c d } \addlyrics { e f g h }")
        first = ir_notes(s)[0]["lyrics"]
        assert sorted((syl["number"], syl["text"]) for syl in first) == [
            (1, "a"),
            (2, "e"),
        ]

    def test_two_lyricsto_stanzas_keep_both(self):
        s = ly(
            r"""<< \new Voice = "v" { c'4 d' e' f' }
            \new Lyrics \lyricsto "v" { a b c d }
            \new Lyrics \lyricsto "v" { e f g h } >>"""
        )
        assert sorted(syllables(s)[0]) == ["a", "e"]

    def test_lyricsto_targets_the_named_voice(self):
        s = ly(
            r"""\new Staff << \new Voice = "s" { \voiceOne e''4 f'' g'' a'' }
            \new Voice = "a" { \voiceTwo c''2 d'' }
            \new Lyrics \lyricsto "s" { sa sb sc sd }
            \new Lyrics \lyricsto "a" { aa ab } >>"""
        )
        by_pitch = {n.pitch.name: n.lyrics for p in s.iter_parts() for n in p.notes}
        assert by_pitch["C5"] == ["aa"] and by_pitch["D5"] == ["ab"]
        assert by_pitch["F5"] == ["sb"]

    def test_musicxml_elision_keeps_both_words(self):
        x = xml_part(
            xml_note(
                "C",
                '<lyric number="1"><syllabic>single</syllabic><text>my</text>'
                "<elision>‿</elision><syllabic>single</syllabic><text>a</text></lyric>",
            )
        )
        s = lytk.from_musicxml_string(x)
        assert syllables(s)[0] == ["my‿a"]

    def test_musicxml_named_verse_is_not_verse_one(self):
        x = xml_part(xml_note("C", lyric("one") + lyric("refrain", "chorus")))
        numbers = {
            syl["number"] for syl in ir_notes(lytk.from_musicxml_string(x))[0]["lyrics"]
        }
        assert len(numbers) == 2

    def test_midi_syllabic_follows_the_previous_hyphen(self):
        s = ly(r"\relative c' { c4 d e f } \addlyrics { Hal -- le -- lu jah }")
        back = lytk.from_midi_bytes(lytk.to_midi_bytes(s))
        kinds = [syl["syllabic"] for n in ir_notes(back) for syl in n["lyrics"]]
        assert kinds == ["Begin", "Middle", "End", "Single"]

    def test_humdrum_text_spine_is_read(self):
        krn = "**kern\t**text\n*M4/4\t*\n4c\tHal-\n4d\t-le\n4e\tlu\n4f\tjah\n*-\t*-\n"
        s = lytk.from_humdrum_string(krn)
        assert syllables(s) == [["Hal"], ["le"], ["lu"], ["jah"]]


class TestLyricSyllables:
    def test_a_note_says_each_verse_and_how_its_syllable_sits(self):
        s = ly(r"""{ c'4 d' e' f' } \addlyrics { Hal -- le my~a __ jah }
                \addlyrics { \set stanza = "2." a b c d }""")
        notes = s.iter_parts()[0].notes
        first = notes[0].lyric_syllables
        assert [
            (syl["verse"], syl["text"], syl["syllabic"], syl["name"]) for syl in first
        ] == [
            (1, "Hal", "begin", None),
            (2, "a", "single", "2."),
        ]
        third = notes[2].lyric_syllables[0]
        assert (third["text"], third["elision"], third["extend"]) == (
            "my\u203fa",
            True,
            True,
        )


class TestLyricsWriting:
    LATE_VERSE = xml_part(
        xml_note("C", lyric("one"))
        + xml_note("D", lyric("two"))
        + xml_note("E", lyric("three") + lyric("late", "2"))
        + xml_note("F", lyric("four"))
    )

    def test_lilypond_keeps_a_late_starting_verse_in_place(self):
        s = lytk.from_musicxml_string(self.LATE_VERSE)
        back = ly(lytk.to_lilypond(s))
        assert [("late" in t) for t in syllables(back)] == [False, False, True, False]

    def test_abc_keeps_a_late_starting_verse_in_place(self):
        s = lytk.from_musicxml_string(self.LATE_VERSE)
        back = lytk.from_abc_string(lytk.to_abc(s))
        assert [("late" in t) for t in syllables(back)] == [False, False, True, False]

    def test_lilypond_slurred_notes_keep_their_syllables(self):
        # Passes today only because the reader and the writer share one wrong
        # melisma rule; a guard for M3, which fixes the reader.
        x = xml_part(
            xml_note("C", lyric("a"))
            + xml_note("D", '<notations><slur type="start"/></notations>' + lyric("b"))
            + xml_note("E", '<notations><slur type="stop"/></notations>' + lyric("c"))
            + xml_note("F", lyric("d"))
        )
        back = ly(lytk.to_lilypond(lytk.from_musicxml_string(x)))
        assert syllables(back) == [["a"], ["b"], ["c"], ["d"]]


# ---------------------------------------------------------------------------
# F1b, F5 — text and MusicXML validity
# ---------------------------------------------------------------------------


class TestText:
    def test_musicxml_escapes_text(self):
        s = ly(r'\header { title = "Tom & Jerry <live>" } { c4 }')
        back = lytk.from_musicxml_string(lytk.to_musicxml(s))
        assert back.title == "Tom & Jerry <live>"

    def test_musicxml_always_declares_divisions(self):
        assert "<divisions>" in lytk.to_musicxml(ly("{ c'4 d' e' f' }"))

    def test_lilypond_church_mode_keeps_its_tonic(self):
        back = ly(lytk.to_lilypond(ly(r"{ \key d \dorian d'1 }")))
        keys = [
            m.key_signature
            for p in back.iter_parts()
            for m in p.measures
            if m.key_signature
        ]
        assert keys[0] == (0, "dorian")

    def test_text_scripts_are_read(self):
        s = ly(r'{ c4^"dolce" d_"rit." }')
        assert "dolce" in s.to_json() and "rit." in s.to_json()


# ---------------------------------------------------------------------------
# F2 — stems and beams
# ---------------------------------------------------------------------------


class TestStemsAndBeams:
    def test_a4_in_treble_is_stem_up(self):
        assert stems(lytk.to_musicxml(ly("{ a'4 }"))) == ["up"]

    def test_a_beam_group_shares_one_direction(self):
        xml = lytk.to_musicxml(ly("{ g'8 a' b' c'' }"))
        assert len(set(stems(xml))) == 1

    def test_voices_point_away_from_each_other(self):
        xml = lytk.to_musicxml(ly(r"<< { c''4 d'' } \\ { a'4 b' } >>"))
        notes = re.findall(r"<note>.*?</note>", xml, re.S)
        by_voice = {
            re.search(r"<voice>(\d+)", n).group(1): re.search(r"<stem>(\w+)", n).group(
                1
            )
            for n in notes
            if "<stem>" in n
        }
        assert by_voice["1"] == "up" and by_voice[max(by_voice)] == "down"

    def test_lilypond_output_adds_no_stem_commands(self):
        assert r"\stem" not in lytk.to_lilypond(ly("{ a'8 b' c'' d'' }"))

    def test_three_eight_eighths_are_beamed(self):
        assert "<beam" in lytk.to_musicxml(ly(r"{ \time 3/8 c''8 d'' e'' }"))

    def test_no_beam_over_a_rest(self):
        xml = lytk.to_musicxml(ly(r"{ \time 6/8 c''8 r d'' e'' f'' g'' }"))
        first = re.findall(r"<note>.*?</note>", xml, re.S)[0]
        assert "<beam" not in first

    def test_abc_beams_by_spacing(self):
        assert "cdef" in lytk.to_abc(ly("{ c''8 d'' e'' f'' g''2 }"))

    def test_retrograde_flips_beams(self):
        out = lytk.to_lilypond(lytk.retrograde(ly("{ c''8[ d''] e''4 }")))
        body = out.split("{", 1)[1]
        assert body.index("[") < body.index("]")


# ---------------------------------------------------------------------------
# F3, F4 — MusicXML positions and chord symbols
# ---------------------------------------------------------------------------


class TestPositions:
    def test_second_voice_starts_on_beat_two(self):
        # MusicXML test suite 46e: "Voice 2 should start at 2nd beat".
        s = lytk.from_musicxml(
            "tests/fixtures/xml/46e-PickupMeasure-SecondVoiceStartsLater.xml"
        )
        onsets = {
            (r[0], r[2]) for r in lytk.to_note_array(s.to_music_document()).tolist()
        }
        assert (960, 60) in onsets

    def test_chord_symbols_keep_their_beat(self):
        chord = "<harmony><root><root-step>{}</root-step></root><kind>major</kind></harmony>"
        x = xml_part(
            chord.format("C")
            + xml_note("C", dur=2)
            + chord.format("G")
            + xml_note("D", dur=2)
        )
        harmonies = ir_measures(lytk.from_musicxml_string(x))[0]["harmonies"]
        assert [h["offset"] for h in harmonies] == [0, 8]  # 4 per quarter: beat 3

    def test_lilypond_chordmode_puts_the_duration_first(self):
        s = ly(r"<< \new ChordNames \chordmode { c2 g2:7 } \new Staff { c'2 d'2 } >>")
        assert "g2:7" in lytk.to_lilypond(s)

    def test_lilypond_to_lilypond_keeps_chord_symbols(self):
        # The CLI's ly -> ly route (the music tree) used to drop them.
        src = r"<< \new ChordNames \chordmode { c2 g2:7 } \new Staff { c'2 d'2 } >>"
        assert "g2:7" in lytk.to_lilypond_music(lytk.from_lilypond_music_string(src))

    def test_abc_fine_is_text_and_nc_a_no_chord(self):
        s = lytk.from_abc_string('X:1\nL:1/4\nK:C\n"Fine"C "N.C."D "G7b9"E F|\n')
        kinds = [h["kind"] for m in ir_measures(s) for h in m["harmonies"]]
        assert kinds == ["none", "dominant"]


# ---------------------------------------------------------------------------
# F6, F7 — ABC and Humdrum writers
# ---------------------------------------------------------------------------


class TestWriters:
    def test_abc_keeps_a_staff_of_voices(self):
        s = ly(r"\new Staff << { c''4 d'' e'' f'' } \\ { c'4 d' e' f' } >>")
        back = lytk.from_abc_string(lytk.to_abc(s))
        assert sum(len(p.notes) for p in back.iter_parts()) == 8

    def test_abc_closes_a_diminuendo_as_one(self):
        abc = lytk.to_abc(ly(r"{ c'4\> d' e'\! f' }"))
        assert "!>)!" in abc and "!<)!" not in abc

    def test_humdrum_writes_each_staffs_clef(self):
        s = ly(
            r"\new PianoStaff << \new Staff { c''1 } \new Staff { \clef bass c1 } >>"
        )
        assert "*clefF4" in lytk.to_humdrum(s)

    def test_humdrum_rests_a_voice_absent_from_a_bar(self):
        krn = lytk.to_humdrum(
            ly(r"\new Staff { c''2 << { d''2 } \\ { b'2 } >> | c''1 }")
        )
        assert "*staff1" in krn and "2ryy" in krn and "1ryy" in krn

    def test_a_piano_tempo_is_written_once(self):
        s = ly(
            r"\new PianoStaff << \new Staff { \tempo 4 = 90 c''1 } \new Staff { \clef bass c1 } >>"
        )
        assert lytk.to_lilypond(s).count(r"\tempo") == 1

    def test_marks_keep_their_beat_and_text(self):
        out = lytk.to_lilypond(
            ly(r"""{ c''4 d'' \mark \default e'' f'' | \mark "Intro" g''1 }""")
        )
        assert r"d''4 \mark \default e''4" in out and r'\mark "Intro"' in out

    def test_lilypond_to_lilypond_keeps_marks_on_their_notes(self):
        xml = xml_part(
            xml_note("C", dur=2)
            + "<direction><direction-type><dynamics><ff/></dynamics>"
            "</direction-type></direction>" + xml_note("D", dur=2)
        )
        out = lytk.to_lilypond_music(lytk.from_musicxml_string(xml).to_music_document())
        back = ir_notes(lytk.from_lilypond_string(out))
        assert [n["dynamics"] for n in back] == [[], []]  # a direction at D's onset
        assert out.index("c'2") < out.index(r"<>\ff") < out.index("d'2")

    def test_musicxml_da_capo_is_written_once(self):
        xml = xml_part(
            xml_note("C", dur=4) + "<direction><direction-type><words>D.C. al Fine"
            '</words></direction-type><sound dacapo="yes"/></direction>'
        )
        s = lytk.from_musicxml_string(xml)
        assert lytk.to_musicxml(s).count("D.C.") == 1
        assert lytk.to_lilypond(s).count("D.C.") == 1

    def test_musicxml_keeps_placement_and_mid_bar_clefs(self):
        x = lytk.to_musicxml(ly(r"{ c''4^. d''_( e'') \clef bass f4 }"))
        assert '<staccato placement="above"/>' in x and 'placement="below"' in x
        bar = re.search(r"<measure.*?</measure>", x, re.S).group(0)
        assert re.findall(r"<(note|clef)\b", bar) == [
            "note",
            "note",
            "note",
            "clef",
            "note",
        ]

    def test_abc_writes_clefs_fingerings_and_bowings(self):
        abc = lytk.to_abc(
            ly(
                r"\new PianoStaff << \new Staff { c''1 } \new Staff { \clef bass c1 } >>"
            )
        )
        assert "clef=bass" in abc
        abc = lytk.to_abc(ly(r"{ c''4-1 d''\upbow e''\downbow f'' }"))
        assert "!1!" in abc and "!upbow!" in abc and "!downbow!" in abc

    def test_midi_parts_with_other_programs_get_other_channels(self):
        xml = (
            """<?xml version="1.0"?><score-partwise version="4.0"><part-list>
            <score-part id="A"><part-name>Fl</part-name><midi-instrument id="A-I"><midi-channel>1</midi-channel>
            <midi-program>74</midi-program></midi-instrument></score-part>
            <score-part id="B"><part-name>Ch</part-name><midi-instrument id="B-I"><midi-channel>1</midi-channel>
            <midi-program>53</midi-program></midi-instrument></score-part></part-list>"""
            + "".join(
                f'<part id="{p}"><measure number="1"><attributes><divisions>1</divisions></attributes>'
                f'{xml_note("C", dur=4)}</measure></part>'
                for p in "AB"
            )
            + "</score-partwise>"
        )
        back = lytk.from_midi_bytes(lytk.to_midi_bytes(lytk.from_musicxml_string(xml)))
        # Both asked for channel 1: one program per channel.
        assert sorted(p.midi_channel for p in back.iter_parts()) == [1, 2]


# ---------------------------------------------------------------------------
# F8, F13 — representations
# ---------------------------------------------------------------------------


class TestRepresentations:
    def test_dynamics_stay_in_their_part(self):
        s = ly(r"<< \new Staff { c'4\f d' e' f'\pp } \new Staff { c4 d e f } >>")
        rows = lytk.to_note_array(s.to_music_document()).tolist()
        low = [r[3] for r in rows if r[2] < 60]
        assert 33 not in low

    def test_note_arrays_use_sounding_pitch(self):
        x = xml_part(
            xml_note("E"),
            "<transpose><diatonic>-1</diatonic><chromatic>-2</chromatic></transpose>",
        )
        rows = lytk.to_note_array(lytk.from_musicxml_string(x).to_music_document())
        assert rows[0][2] == 62

    def test_note_arrays_read_direction_dynamics(self):
        x = xml_part(
            "<direction><direction-type><dynamics><ff/></dynamics></direction-type></direction>"
            + xml_note("C")
        )
        rows = lytk.to_note_array(lytk.from_musicxml_string(x).to_music_document())
        assert rows[0][3] > 64

    def test_grace_chords_take_no_time(self):
        rows = lytk.to_note_array(
            ly(r"{ \grace <d' f'>8 c'4 d'4 }").to_music_document()
        )
        assert [r[0] for r in rows.tolist() if r[2] == 60] == [0]


# ---------------------------------------------------------------------------
# F9 — transforms
# ---------------------------------------------------------------------------


class TestTransforms:
    def test_retrograde_moves_key_signatures(self):
        r = lytk.retrograde(ly(r"{ \key g \major g'1 | \key f \major f'1 }"))
        first = [m.key_signature for p in r.iter_parts() for m in p.measures][0]
        assert first == (-1, "major")

    def test_transpose_prefers_fewer_accidentals(self):
        t = lytk.transpose(ly(r"{ \key g \major g'1 }"), 6)
        keys = [
            m.key_signature
            for p in t.iter_parts()
            for m in p.measures
            if m.key_signature
        ]
        assert keys[0][0] == -5


# ---------------------------------------------------------------------------
# F12, F13 — LilyPond constructs read wrong
# ---------------------------------------------------------------------------


class TestLilyPondConstructs:
    def test_partial_multiplier(self):
        s = ly(r"{ \time 3/4 \partial 8*3 c''8 d'' e'' | f''2. | }")
        first = s.iter_parts()[0].measures[0]
        assert len(first.notes) == 3

    def test_time_with_beat_structure(self):
        s = ly(r"{ \time 3,2 5/8 c''8 d'' e'' f'' g'' }")
        assert s.iter_parts()[0].measures[0].time_signature == ("5", 8, None)

    def test_after_grace(self):
        s = ly(r"{ \afterGrace c'2 { d'16 e' } f'4 g'4 }")
        first = s.iter_parts()[0].notes[0]
        assert not first.is_grace and lytk.info(s)["duration_quarters"] == 4.0

    def test_single_note_tremolo(self):
        s = ly(r"{ \repeat tremolo 8 c'32 d'4 }")
        assert [n.pitch.name for p in s.iter_parts() for n in p.notes] == ["C4", "D4"]
