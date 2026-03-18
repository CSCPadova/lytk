

                                %                   "BETHENA"
                                %                by SCOTT JOPLIN
                                %
                                %  Please see "header.ly" for more information

\language "english"
% === BEGIN INCLUDE: header.ly ===

\header {
                                % LILYPOND HEADERS
    head =  "0.13 (08 March 2015)"

    %%    dedication = "Dedication"
    title = "Bethena"
    subtitle = "A Concert Waltz"
    %%    subsubtitle = "Subsubtitle"

    composer = "Scott Joplin (1868-1905)"
    %%    opus = "Opus"
    %%    arranger = "Arranger"

    %%    poet = "Poet"
    %%    texttranslator = "Translator"
    %%    meter = "meter"

    %%    instrument = "Instrument"
    %%    piece = "Piece"

                                % LILYPOND FOOTERS
    license = "Public Domain"
    %footer = "0.12 (11 Oct 2004)"
    %%    tagline = "Tagline"

                                % MUTOPIA HEADERS
    mutopiatitle = "Bethena"
    mutopiacomposer = "JoplinS"
    mutopiainstrument = "Piano"
    date = "1905"
    source = "Original Manuscript"
    style = "Jazz"
    enteredby = "Magnus Lewis-Smith"
    maintainer = "Magnus Lewis-Smith"
    maintainerEmail = "mlewissmith@users.sourceforge.net"
    maintainerWeb = "http://magware.sourceforge.net/"

 footer = "Mutopia-2015/03/25-463"
 copyright =  \markup { \override #'(baseline-skip . 0 ) \right-column { \sans \bold \with-url #"http://www.MutopiaProject.org" { \abs-fontsize #9  "Mutopia " \concat { \abs-fontsize #12 \with-color #white \char ##x01C0 \abs-fontsize #9 "Project " } } } \override #'(baseline-skip . 0 ) \center-column { \abs-fontsize #11.9 \with-color #grey \bold { \char ##x01C0 \char ##x01C0 } } \override #'(baseline-skip . 0 ) \column { \abs-fontsize #8 \sans \concat { " Typeset using " \with-url #"http://www.lilypond.org" "LilyPond" " by " \maintainer " " \char ##x2014 " " \footer } \concat { \concat { \abs-fontsize #8 \sans{ " Placed in the " \with-url #"http://creativecommons.org/licenses/publicdomain" "public domain" " by the typesetter " \char ##x2014 " free to distribute, modify, and perform" } } \abs-fontsize #13 \with-color #white \char ##x01C0 } } }
 tagline = ##f
}


%{
BUGLIST
*	http:
*	category:  projects/lily
*	group:     sources/lily/joplin_bethena


FEATURE REQUEST
*	http:
*	category:  projects/lily
*	group:     sources/lily/joplin_bethena

Some interesting Scott Joplin links:
*    http:
*    http:
*    http:
%}
% === END INCLUDE: header.ly ===

barRest =  { \skip 1*3/4 }
tenuto = \markup { \italic ten. }

paperOFF = { \set Score.skipTypesetting = ##t }
paperON = { \set Score.skipTypesetting = ##f }

% === BEGIN INCLUDE: intro.ly ===

introSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

introRH = \relative a'' {
    a8 g4 b8 a4 | a,8 g4 b8 a4 |
    R1*3/4 | R1*3/4 |
    <e c g>2. | <ef c g> |
    <d c fs,> | <c fs a> |
}


introLH = \relative a {
    R1*3/4 | R1*3/4 |
    a8 g4 b8 a4 | a,8 g4 b8 a4 |
    a8 g4 b8 a4 | a8 c4 b8 a4 |
    d2. | <d d,> |
}

introSuper =  {
    s4^\markup { \center-column { \line {Valse Tempo} \line { \smaller TEMA} } } s s |
    \barRest |
    \barRest |
    \barRest |
    s^\markup{ rit. poco a poco } s s |
    \barRest |
    \barRest |
    \barRest |
}

introDynamics =  {
    s4\mp s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

introSub =  {
    \introSilent
}
% === END INCLUDE: intro.ly ===
% === BEGIN INCLUDE: partOne.ly ===

partOneSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest | \break
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partOneRHvI = \relative c'' {
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d4 |
    g,8 b4 g8 b4 |
    a2. |
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d( d,) |
    \once \override Slur.positions = #'(1 . 3.5)
    cs8( <g' b>4)
    \once \override Slur.positions = #'(1 . 3.5)
    c,8( <fs a>4) |
    \once \override Slur.positions = #'(3 . 3.5)
    g2( b4) |
}

partOneRHvII = \relative c' {
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 <g b>4 |
    cs,2 <cs g'>4 |
    <c fs>2. |
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 r4 |
    cs,4. c8 c4 |
    <b d>2. |
}

partOneRH =  {
    <<
        \partOneRHvI \\
        \partOneRHvII
    >>
}

partOneLHvI =  \relative g, {
    \stemNeutral
    g4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    \stemUp
    r4 <fs a> r4 |
    \stemNeutral
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <a a,> <e' g a> <a, a,> |
    \stemUp
    r4 <d fs a> <d fs a> |
    \stemNeutral
    g,4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    \stemUp
    r4 <fs a> r4 |
    \stemNeutral
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <e e,> <a, a,> <d d,> |
    <g g,>2. |
}

partOneLHvII =  \relative d {
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
    <d d,>2. |
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
    \barRest
}

partOneLH =  {
    <<
        \partOneLHvI \\
        \partOneLHvII
    >>
}

partOneSuper =  {
    s4^\markup{ \column { \line {a tempo} \line { \smaller {Valse cantabile} } } } s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partOneDynamics =  {
    \once \override DynamicText.extra-offset = #'(3 . 1)
    s4\mp s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partOneSub =  {
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    \barRest |
}


segueOneSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueOneRHvI =  \relative b' {
    b4( a2^\tenuto) |
    <g cs,>2.^\tenuto |
    \once \override Slur.positions = #'(2.5 . 2.5)
    fs4( f2^\tenuto) |
    f'2.\arpeggio^\tenuto |
}
segueOneRHvII =  \relative b {
    <b ds>2. |
                                % [mils] this note is a bit too close to the others
    \once\override Voice.NoteColumn.force-hshift = #1.2
    bf2( a4) |
    \once \override Slur.positions = #'(-5.5 . -5)
    d2( ef4) |
    <ef a c>2.\arpeggio
}
segueOneRHvIII =  \relative a {
    s1*3/4 |
    s1*3/4 |
                                % [mils] this note is a bit too close to the others
    \once\override Voice.NoteColumn.force-hshift = #1.2
    a2. |
    s1*3/4 |
}

segueOneRH =  {
    <<
        \segueOneRHvI \\
        \segueOneRHvII \\
        \segueOneRHvIII
    >>
}

segueOneLHvI =  \relative fs {
    fs2( f4) | % [mils] Expect warning 'clashing notecolumns'
    e2( ef4) | % [mils] Expect warning 'clashing notecolumns'
    d2( c4) |  % [mils] Expect warning 'clashing notecolumns'
    f,2.^\tenuto |     % [mils] Expect warning 'clashing notecolumns'
}

segueOneLHvII =  {
    \transpose c c, \segueOneLHvI
}

segueOneLH =  {
    <<
                                % \applyContext #(lambda (x) (display "\n[mils] expect warnings:  Too many clashing notecolumns.\n"))
        \stemUp
        \segueOneLHvI \\
        \segueOneLHvII
        \stemNeutral
    >>
}

segueOneSuper =  {
    \segueOneSilent
}


segueOneSub =  {
    \segueOneSilent
}

segueOneDynamics =  {
    \segueOneSilent
}
% === END INCLUDE: partOne.ly ===
% === BEGIN INCLUDE: partTwo.ly ===

partTwoSilent =  {
    \repeat volta 2 {
        \barRest |
        \barRest | \break
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest | \break
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest | \break
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        {\barRest |}
        {\barRest |}
    }
}
partTwoRHvI =  \relative d' {
    \repeat volta 2 {
        \stemNeutral
        <d bf' d>8 <g g'>4 <f f'>8 <g g'> <a a'> |
        \stemUp
        <bf bf'>2. |
        \stemNeutral
        <d d'>8 <c c'>4 <f f'>8 <e e'> <ef ef'> |
        \stemUp
        <d d'>2. |
        \stemNeutral
        <bf bf'>8 <ef g>4 <bf bf'>8 <bf cs g'>4 |
        <bf d g>8 <bf d f>4 <bf d g>8 <f b d>4 |
        \stemUp
        d'8 bf4 d8 c4 |
        c2. |
        \stemNeutral
        <d, bf' d>8 <g g'>4 <f f'>8 <g g'> <a a'> |
        \stemUp
        <bf bf'>2. |
        \stemNeutral
        <d d'>8 <c c'>4 <f f'>8 <e e'> <ef ef'> |
        \stemUp
        <d d'>2. |
        \stemNeutral
        <bf bf'>8 <ef g>4 <bf bf'>8 <bf cs g'>4 |
        <bf d g>8 <bf d f>4 <bf d g>8 <f b d>4 |
        \stemUp
        d'8 bf4 d8 c4
        \stemNeutral
    }
    \alternative {
        {
            \stemUp
            <d, f bf>4 f8 bf c cs |
            \stemNeutral
        }
        {
            <d, f bf>4 a'8 af g4 |
        }
    }
}

partTwoRHvII =  \relative d'' {
    \repeat volta 2 {
        \barRest |
        r4 <d f> <d f> |
        \barRest |
        r4 <f bf> <f bf> |
        \barRest |
        \barRest |
        <bf, ef,>2 <bf ef,>4
        <a ef>2. |
        \barRest |
        r4 <d f> <d f> |
        \barRest |
        r4 <f bf> <f bf> |
        \barRest |
        \barRest |
        <bf, ef,>2 <bf ef,>4
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}

partTwoRH =  {
    <<
        \partTwoRHvI \\
        \partTwoRHvII
    >>
}

partTwoLHvI = \relative c {
    \repeat volta 2 {
        <bf bf,>4 <f' bf d>8[ <f f,>] <e e,>[ <ef ef,>] |
        <d d,>4 <f bf d> <f bf d> |
        <f f,> <f a ef'>8[ <f f,>] <g g,>[ <a a,>] |
        <bf bf,>4 <f bf d> <f bf d> |
        <ef ef,> <g bf ef> <e e,> |
        <f f,> <f bf d> <g g,> |
        <c, c,> <g' g,> <gf gf,> |
        <f f,> <a c> <a c> |
        <bf, bf,>4 <f' bf d>8[ <f f,>] <e e,>[ <ef ef,>] |
        <d d,>4 <f bf d> <f bf d> |
        <f f,> <f a ef'>8[ <f f,>] <g g,>[ <a a,>] |
        <bf bf,>4 <f bf d> <f bf d> |
        <ef ef,> <g bf ef> <e e,> |
        <f f,> <f bf d> <g g,> |
        <c, c,> <g' bf c> <f f,> |
    }
    \alternative {
        { <bf bf,> r r | }
        { <bf bf,> a8 af g4 | }
    }
}

partTwoLH =  {
    \partTwoLHvI
}

partTwoSuper =  {
    \partTwoSilent
}

partTwoDynamics =  {
    \repeat volta 2 {
        s4\f s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}

partTwoSub =  {
    \repeat volta 2 {
        \barRest |
        s4 \sustainOn s s \sustainOff |
        \barRest |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s s \sustainOff |
        \barRest |
        s4 \sustainOn s s \sustainOff |
        \barRest |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s \sustainOff s |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}


segueTwoSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueTwoRHvI =  \relative c'' {
    \stemUp
    gf8 f4 e8 ef4 |
    d <g bf d> <fs a d> |
    <f af d> <e g cs> <ef gf c> |
    <d f gs b> <d f gs b> <d f gs b> |
    <cs g' bf>2 <c g' a>4 |
    <c fs a>2.~ |
    <c fs a>2.^\tenuto |
    \stemNeutral
}

segueTwoRH =  {
    \segueTwoRHvI
}

segueTwoLHvI =  \relative c' {
    gf8 f4 e8 ef4 |
    d r r |
    R1*3/4 |
    <d d,>4 <d d,> <d d,> |
    <e e,>2 <ef ef,>4 |
    <d d,>2 a4 |
    <<
        { d,2.^\tenuto } \\
        {
                                % [mils] make this bar a little less crowded
            \override Rest.transparent = ##t
            r4 r2
            \revert Rest.transparent
        }
    >> |
}

segueTwoLH =  {
    \segueTwoLHvI
}

segueTwoSuper =  {
    \segueTwoSilent
}

segueTwoDynamics =  {
    \barRest |
    \barRest |
    \barRest |
    s4\< s s\! |
    s\f s s |
    \barRest |
    \once \override Hairpin.extra-offset = #'(-1 . 0)
    s4\> s s\! |
}
segueTwoSub =  {
    \segueTwoSilent
}
% === END INCLUDE: partTwo.ly ===
% === BEGIN INCLUDE: partThree.ly ===

partThreeSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest | \break
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partThreeRHvI = \relative c'' {
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d4 |
    g,8 b4 g8 b4 |
    a2. |
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d( d,) |
    \once \override Slur.positions = #'(1 . 3.5)
    cs8( <g' b>4)
    \once \override Slur.positions = #'(1 . 3.5)
    c,8( <fs a>4) |
    g2. |
}

partThreeRHvII = \relative c' {
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 <g b>4 |
    cs,2 <cs g'>4 |
    <c fs>2. |
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 r4 |
    cs,4. c8 c4 |
    <b d>2. |
}

partThreeRH =  {
    <<
        \partThreeRHvI \\
        \partThreeRHvII
    >>
}

partThreeLHvI =  \relative g, {
    \stemNeutral
    g4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    \stemUp
    r4 <fs a> r4 |
    \stemNeutral
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <a a,> <e' g a> <a, a,> |
    \stemUp
    r4 <d fs a> <d fs a> |
    \stemNeutral
    g,4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    \stemUp
    r4 <fs a> r4 |
    \stemNeutral
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <e e,> <a, a,> <d d,> |
    <g g,>4 d g, |
}

partThreeLHvII =  \relative d {
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
    <d d,>2. |
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
    \barRest
}

partThreeLH =  {
    <<
        \partThreeLHvI \\
        \partThreeLHvII
    >>
}

partThreeSuper =  {
    s4^\markup { \smaller cantabile} s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partThreeDynamics =  {
    s4\mp s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partThreeSub =  {
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    \barRest |
}

segueThreeSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest | \break
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueThreeRHvI =  \relative g'' {
    r4 g8( d' c b) |
    r4 e,8( b' a gs) |
    r4 ds8( gs as b) |
    r4 e,8( b' a gs) |
    r4 cs,8( gs' fs es) |
    r4 c8( f g af) |
    <df,, f af>4 <d f af> <d f a> |
    bf'2.^\tenuto |
}

segueThreeRHvII =  \relative g' {
    g2. |
    gs |
    gs |
    gs |
    <es gs> |
    <f af> |
    s |
    <d f>4( <c e>2) |
}

segueThreeRH =  {
    <<
        \segueThreeRHvI \\
        \segueThreeRHvII
    >>
}


segueThreeLHvI =  \relative f {
    \stemNeutral
    \barRest |
    \barRest |
    \barRest |
    <d b'>2. |
    <cs b'>^\tenuto |
    <c c'> |
    cf4 bf a8 d |
    \slurDown
    g,4( c2) |
    \slurNeutral
}

segueThreeLHvII =  \relative f {
    <f b d>2.^\tenuto |
    <e b' d>^\tenuto |
    <ds b' ds> |
    \showStaffSwitch
    \change Staff = "rh"
    e' | % [mils] Expect warning 'clashing notecolumns'
    \change Staff = "lh"
    \hideStaffSwitch
    \barRest
    \barRest
    \barRest
    \barRest
}

segueThreeLH =  {
    <<
                                % \applyContext #(lambda (x) (display "\n[mils] expect warnings:  Too many clashing notecolumns.\n"))
        \segueThreeLHvI \\
        \segueThreeLHvII
    >>
}


segueThreeSuper =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    s4^\markup{rit.} s s |
    \barRest |
}

segueThreeDynamics =  {
    \once \override DynamicText.extra-offset = #'(-2 . -1.5)
    s4\f s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueThreeSub =  {
    s8 \sustainOn s s s s s \sustainOff |
    s8 \sustainOn s s s s s \sustainOff |
    s8 \sustainOn s s s s s \sustainOff |
    s8 \sustainOn s s s s s \sustainOff |
    s8 \sustainOn s s s s s \sustainOff |
    s8 \sustainOn s s s s s \sustainOff |
    \barRest |
    \barRest |
}
% === END INCLUDE: partThree.ly ===
% === BEGIN INCLUDE: partFour.ly ===

partFourSilent =  {
    \repeat volta 2 {
        \bar ".|:-||"
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest | \break
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest | \break
    }
    \alternative {
        {
            \barRest |
            \barRest |
            \barRest |
            \barRest |
            \barRest | \break
        }
        {
            \barRest |
        }
    }
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}


partFourRHvI =  \relative c' {
    \repeat volta 2 {
        \stemNeutral
        <c f a>4( <c e bf'> <d f b>) |
        c'8( <f a>4) c8( <f a>4) |
        bf,8( <e g>4) bf8( <bf e g>4) |
        <a d f>2 <a a'>4 |
        <a c ds a'>8 <a c ds a'>4 <a c ds f>8 <a c ds f>4 |
        <a c e a>8 <a c e a>4 <a c e>8 <a c e>4 |
        <gs e'>8 <gs e'>4 <gs e'>8 <gs e'>4 |
        a4( c bf) |
        <c, f a>4( <c e bf'> <d f b>) |
        c'8( <f a>4) c8( <f a>4) |
        bf,8( <e g>4) bf8( <bf e g>4) |
    }
    \alternative {
        {
            <a d f>2 a'8( b) |
            \stemUp
            c8 a4 c8 a4 |
            c8 g4 c8 g4 |
            g8 g4 g8 g4 |
            \stemNeutral
            <c e, c>8( bf g e c bf) |
        }
        {
            <a d f>2 d8( e) |
        }
    }
    <f f,>8 <bf, d>4 <f f'>8 <f gs d'>4 |
    <f a d>8 <f a c>4 <f a d>8 <c fs a>4 |
    a'8 f4 a8 g4 |
    \stemUp
    f f8( f' f4) |
    \stemNeutral
}

partFourRHvII =  {
    \repeat volta 2 {
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        \relative ds'' {
            \barRest |
            ds2.( |
            e) |
            <b f'>2 <b f'>4 |
            \barRest |
        }
        {
            \barRest |
        }
    }
    \relative f' {
      \barRest |
      \barRest |
      <f b,>2 <e bf>4 |
                                % [mils] these slurs should be on both note-heads
                                % [mils] (crossing staves) throughout this bar
      <c a>(
      \slurUp
      \change Staff = "lh"
      \stemUp
      <df bf>)( <c a>) |      % [mils] slurs on both note-heads as above
      \change Staff = "rh"
      \stemNeutral\slurNeutral
    }
}


partFourRH =  {
    <<
        \partFourRHvI \\
        \partFourRHvII
    >>
}


partFourLHvI =  {
    \repeat volta 2 \relative f {
        \stemNeutral
        <f f,>4 <g g,> <gs gs,> |
        <a a,> <a c f> <a c f> |
        \stemUp
        r4 <bf c e> r |
        r <a d f> <a d f> |
        \stemNeutral
        <f f,>4 <a c ds> <a c ds> |
        <e e,> <a c e> <a c e> |
        <e e,> <gs d' e> <gs d' e> |
        <a c e>2 <g c e>4 |
        <f f,>4 <g g,> <gs gs,> |
        <a a,> <a c f> <a c f> |
        \stemUp
        r4 <bf c e> r |
        \stemNeutral
    }
    \alternative {
        \relative a {
            \stemUp
            r4 <a d f> r |
            r <a c ds> <a c ds> |
            r <g c e> <g c e> |
            \stemNeutral
            <g g,>4 <g b f'> <g b f'> |
            <c, c,> <g' bf c e>2 |
        }
        \relative a {
            \stemUp
            r4 <a d f> r |
        }
    }
    \relative a {
      r <f bf d> r |
      r <f a c> r |
      r <d f g> r |
      \stemNeutral
      \barRest |
    }
}

partFourLHvII =  {
    \repeat volta 2 \relative c {
        \barRest |
        \barRest |
        <c c,>2 <cs cs,>4 |
        <d d,>2. |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        <c c,>2 <cs cs,>4 |
    }
    \alternative {
        \relative d {
            <d d,>2. |
            <fs fs,> |
            <g g,> |
            \barRest |
            \barRest |
        }
        \relative d {
            <d d,>2. |
        }
    }
    \relative d {
      <bf bf,>2 <b b,>4 |
      <c c,>2 <d d,>4 |
      <g, g,>2 <c c,>4 |
      <f f,>2. |
    }
}

partFourLH =  {
    <<
        \partFourLHvI \\
        \partFourLHvII
    >>
}

partFourSuper =  {
    \repeat volta 2 {
        s4^\markup{ \column { \line {a tempo} \line { \smaller cantabile} } } s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        s^\markup{rall.} s s |
        \barRest
        s^\markup{a tempo} s s |
        \barRest |
        \barRest |
    }
    \alternative {
        {
            \barRest |
            \barRest |
            \barRest |
            s4^\markup{rit.} s s |
            \barRest |
        }
        {
            \barRest |
        }
    }
    s8 s s s^\markup{rit. poco a poco} s s |
    \revert TextScript.extra-offset
    \barRest |
    \barRest |
    \barRest |
}

partFourDynamics =  {
    \repeat volta 2 {
        s4\f s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
                                % [mils] hugely ugly code to print "dim." instead
                                %        of ">" (decrescendo)
        \once \override TextScript.extra-offset = #'(0 . -1)
        \once \override TextScript.font-series = #'medium
        \once \override Hairpin.transparent = ##t
        s^\markup { \normalsize dim. } \> s s |
        s s s\! |
        s\f s s |
        \barRest |
        \barRest |
    }
    \alternative {
        {
            \barRest |
            \barRest |
            \barRest |
            \barRest |
            \barRest |
        }
        {
            \barRest |
        }
    }
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partFourSub =  {
    \repeat volta 2 {
        \barRest |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s \sustainOff s |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        \barRest |
        \barRest |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s \sustainOff s |
    }
    \alternative {
        {
            s4 \sustainOn s \sustainOff s |
            s4 \sustainOn s s \sustainOff |
            s4 \sustainOn s s \sustainOff |
            s4 \sustainOn s s \sustainOff |
            \barRest |
        }
        {
            s4 \sustainOn s\sustainOff s |
        }
    }
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s \sustainOff s |
    \barRest |
}


segueFourSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueFourRHvI =  \relative f'' {
    f8( e f e ef d) |
    e( ds e ds d df) |
    ef( d ef d df c) |
    cs2 r4\fermata |
 }

segueFourRH =  {
    \segueFourRHvI
}

segueFourLHvI =  \relative as {
    \barRest |
    \barRest |
    \barRest |
    as2 s4 |
}

segueFourLHvII =  \relative gs{
    <gs b d>2.^\tenuto |
    <g bf df>^\tenuto |
    <fs a c>^\tenuto |
    fs4 fs, s4 |
}

segueFourLH =  {
    <<
        <<
            \segueFourLHvI \\
            \segueFourLHvII
        >>
        { s1*3/4*3 s2 r4\fermata }
    >>
}


segueFourSuper =  {
    s4^\markup {a tempo} s s |
    \barRest |
    \barRest |
    \barRest |
}

segueFourDynamics =  {
    \segueFourSilent
}

segueFourSub =  {
    \segueFourSilent
}
% === END INCLUDE: partFour.ly ===
% === BEGIN INCLUDE: partFive.ly ===

partFiveSilent =  {
    \repeat volta 2 {
        \bar ".|:-||"
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}


partFiveRHvI =  \relative d''{
    \repeat volta 2 {
        d8 b4 fs'8 d4 |
        b'8( fs d) b~ b4 |
        as8 fs'4 g8 fs4 |
        b,8 fs'4 g8 fs4 |
        d8 b4 fs'8 d4 |
        b'8[( fs d) b]~ b d |
        <b d>8 <b d>4 <b d>8 <b cs e> <b cs es> |
        <as cs fs>4 cs,^^
        \showStaffSwitch
        \stemUp\change Staff = "lh"
        \acciaccatura es,8 fs4^^ |
        \stemNeutral\change Staff = "rh"
        \hideStaffSwitch
        d''8 b4 fs'8 d4 |
        b'8( fs d) b~ b4 |
        as8 fs'4 g8 fs4 |
        b,8 fs'4 g8 fs b |
        b g4 a8 g fs |
        fs d4 e8 d cs |
        cs as4 g'8 fs4 |
    }
    \alternative {
        { b,4 g^^ fs^^ | }
        { b4 <d, es gs d'> <d fs a c> | }
    }
}

partFiveRH =  {
    \partFiveRHvI
}


partFiveLHvI = \relative b, {
    \repeat volta 2 {
        <b b,>4 <fs' b d> <fs b d> |
        <b, b,>4 <fs' b d> <fs b d> |
        <fs, fs,> <fs' as e'> <fs as e'> |
        <b, b,> <fs' b d> <fs b d> |
        <b, b,>4 <fs' b d> <fs b d> |
        <b, b,>4 <fs' b d> <fs b d> |
        <g g,> <g g,> <g g,> |
        <fs fs,> cs^^
        {
                                % [mils] ugly code to prevent stems from clashing with
                                % [mils] the E#-F# (right-hand).  The slur should be
                                % [mils] above the notes.
            \acciaccatura {
                \once \override Stem.direction = #-1
                \once \override Slur.direction = #1
                es,8
            }
            \stemDown fs4_^ \stemNeutral |
        }
        <b b,>4 <fs' b d> <fs b d> |
        <b, b,>4 <fs' b d> <fs b d> |
        <fs, fs,> <fs' as e'> <fs as e'> |
        <b, b,> <fs' b d> <fs b d> |
        <e e,> <g b e> <g b e> |
        <b, b,> <fs' b d> <fs b d> |
        <fs, fs,> <fs' as e'> <fs as e'> |
    }
    \alternative{
        { <b d> <g g,>^^ <fs fs,>^^ | }
        { <b d> <b b,> <a a,> | }
    }
}

partFiveLH =  {
    \partFiveLHvI
}


partFiveSuper =  {
    \repeat volta 2 {
        s4^\markup{ \smaller cantabile} s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        s4^\markup{rit.} s s |
        \barRest |
        s4^\markup{a tempo} s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}

partFiveDynamics =  {
    \repeat volta 2 {
        s4\p s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        s8\< s s s s s\! |
        s\f s s\> s s s\! |
        s\p s s s s s |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}

partFiveSub =  {
    \repeat volta 2 {
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        \barRest |
        \barRest |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
        s4 \sustainOn s s \sustainOff |
    }
    \alternative {
        { \barRest | }
        { \barRest | }
    }
}
% === END INCLUDE: partFive.ly ===
% === BEGIN INCLUDE: partSix.ly ===

partSixSilent =  {
    \repeat volta 2 {
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | \barRest | }
        { \barRest | }
    }
}

partSixRHvI =  \relative b' {
    \repeat volta 2 {
        \stemUp
        b8 d4 b8 d4 |
        a4. a8([ d fs)] |
        a g4 g8 fs e |
        \stemDown
        <fs d a> d \stemNeutral a4 <c fs, d> |
        \stemUp
        b8 d4 b8 d4 |
        a2 d4 |
        \stemDown
        cs8( bs cs bs cs fs) |
        \stemNeutral
        b,4 <d gs, es d> <c a fs d> |
        \stemUp
        b8 d4 b8 d4 |
        a4. a8([ d fs)] |
        a g4 g8 fs e |
        fs4. d8([ fs a)] |
        b d4 d8 e4 |
        d8( a) fs4. fs8 |
        \stemNeutral
    }
    \alternative {
        \relative a'' {
            \stemUp
            a8 g4 g8 fs e |
            <d a fs>2 <c fs, d>4 |
            \stemNeutral
        }
        \relative a'' {
            \stemUp
            a8 g4 e8 cs4 |
            \stemNeutral
        }
    }
}

partSixRHvII =  \relative d' {
    \repeat volta 2 {
        <d g b>2 <d gs>4 |
        d4. r8 r4 |
        <a' cs>2. |
        \barRest |
        <d, g b>2 <d gs>4 |
        d2. |
        \barRest |
        \barRest |
        <d g b>2 <d gs>4 |
        d4. r8 r4 |
        <a' cs>2. |
        <a d>2 d4 |
        <d g>2 g4 |
        fs2 r4 |
    }
    \alternative {
        \relative c'' {
             <cs a>2 <cs a>8 <cs g> |
            \barRest |
        }
        \relative a' {
             <a cs>2 <g e>4 |
         }
    }
}

partSixRH =  {
    <<
        \partSixRHvI \\
        \partSixRHvII
    >>
}


partSixLHvI =  \relative g {
    \stemNeutral
    \repeat volta 2 {
        <g g,>4 <e e,> <es es,> |
        <fs fs,> <fs a> <fs a> |
        <a, a,> <g' a cs> <g a cs> |
        <d d,> <fs a d> <a a,> |
        <g g,> <e e,> <es es,> |
        \stemUp
        r <fs a> <fs a>
        \stemNeutral
        <g as e'>2 <fs as e'>4 |
        <b d> <b b,> <a a,> |
        <g g,>4 <e e,> <es es,> |
        <fs fs,> <fs a> <fs a> |
        <a, a,> <g' a cs> <g a cs> |
        <d d,> <fs a d> <a a,> |
        <g g,> <b b,> <bf bf,> |
        <a a,> <fs a d> <fs a d> |
    }
    \alternative {
        \relative a, {
            <a a,>4 <g' a cs> <g a cs> |
            <d d,> <fs a d> <a a,> |
        }
        \relative a, {
            \once \override Slur.positions = #'(2.5 . 0)
            a4( a') bf |
        }
    }
}

partSixLHvII =  {
    \repeat volta 2 {
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        <fs fs,>2. |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | \barRest | }
        { \barRest | }
    }
}

partSixLH =  {
    <<
        \partSixLHvI \\
        \partSixLHvII
    >>
}

partSixSuper =  {
    \repeat volta 2 {
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | \barRest | }
        {
            \once \override TextScript.extra-offset = #'(2 . 0)
            s4^\markup{rit.} s s |
        }
    }
}

partSixDynamics =  {
    \repeat volta 2 {
        s4\mf s s |
        s8 s s\< s s s\! |
        s4\f s s |
        s8 s s\> s s s\! |
        s4\mf s s |
        s8\< s s s s s\! |
        s4\f s s |
        s8 s s\> s s s\! |
        s4\mf s s |
        s8 s s\< s s s\! |
        s4\f s s |
        \barRest |
        \barRest |
        \barRest |
    }
    \alternative {
        { \barRest | \barRest | }
        { s4\f s s | }
    }
}

partSixSub =  {
    \repeat volta 2 {
        s4\sustainOn s\sustainOff s |
        \barRest |
        s4\sustainOn s s\sustainOff |
        s4\sustainOn s\sustainOff s |
        s4\sustainOn s\sustainOff s |
        s4\sustainOn s s\sustainOff |
        \barRest |
        \barRest |
        \barRest |
        \barRest |
        s4\sustainOn s s\sustainOff |
        s4\sustainOn s\sustainOff s |
        \barRest |
        s4\sustainOn s s\sustainOff |
    }
    \alternative {
        {
            s4\sustainOn s s\sustainOff |
            \barRest |
        }
        { \barRest | }
    }
}

segueSixSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueSixRHvI =  \relative ds' {
    <ds fs c'>2.^\tenuto |
    <d f b>^\tenuto |
    <cs e bf'>^\tenuto |
    <cs g' bf>4 <c g' a>2^\tenuto |
    <c fs a>2.^\tenuto |
}

segueSixRH =  {
    \segueSixRHvI
}

segueSixLHvI =  \relative a {
    a8( gs a gs g fs) |
    gs( fss gs fss fs f) |
    g( fs g fs f e) |
    <e e,>4 <ef ef,>2^\tenuto |
    <d d,>2.^\tenuto |
}

segueSixLH =  {
    \segueSixLHvI
}


segueSixSuper =  {
    s4^\markup{a tempo} s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

segueSixDynamics =  {
    s4\f s s |
    s4\f s s |
    s8\f\< s s s s s\! |
    s4\ff
    s2\ff |
    s2.\ff |
}

segueSixSub =  {
    \segueSixSilent
}
% === END INCLUDE: partSix.ly ===
% === BEGIN INCLUDE: partSeven.ly ===

partSevenSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest | \break
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest | \break
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partSevenRHvI = \relative c'' {
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d4 |
    g,8 b4 g8 b4 |
    a2. |
    a8 g4 b8 a4 |
    a8 g4 b8 a4 |
    a8 c4 b8 a4 |
    g2. |
    e'8 a4 e8 g4 |
    b,8 e4 b8 d( d,) |
                                % [mils] move the \fermata so that it doesn't
                                % [mils] clash with the slur
    \override Script.padding = #1.2
    \once \override Slur.positions = #'(1 . 3.5)
    cs8( <g' b>4)
    \once \override Slur.positions = #'(1 . 3.5)
    c,8( <fs a>4) \fermata |
    \revert Script.padding
}

partSevenRHvII = \relative c' {
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 <g b>4 |
    cs,2 <cs g'>4 |
    <c fs>2. |
    <b d>2. |
    <cs g'> |
    <c fs>2 <c fs>4 |
    <b e>2. |
    c'2 c4 |
    g2 r4 |
    cs,4. c8
    c4 \fermata |
}

partSevenRH =  {
    <<
        \partSevenRHvI \\
        \partSevenRHvII
    >>
}

partSevenLHvI =  \relative g, {
    g4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    r4 <fs a> r4 |
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <a a,> <e' g a> <a, a,> |
    r4 <d fs a> <d fs a> |
    g,4 <d' g> <d g> |
    <e e,> <e g a> <e g a> |
    r4 <fs a> r4 |
    <e e,>4 <e g> <e g> |
    <c c,> <e g c> <e g c> |
    <d d,> <d g b> <d g b> |
    <e e,> <a, a,> <d d,> \fermata |
}

partSevenLHvII =  \relative d {
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
    <d d,>2. |
    \barRest
    \barRest
    <d d,>2 <ds ds,>4
    \barRest
    \barRest
    \barRest
    \barRest
}

partSevenLH =  {
    <<
        \partSevenLHvI \\
        \partSevenLHvII
    >>
}

partSevenSuper =  {
    s4^\markup { \smaller FINALE} s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    s4^\markup {rit. poco a poco} s s |
    \barRest |
}

partSevenDynamics =  {
    s4\mf s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

partSevenSub =  {
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s s \sustainOff |
    s4 \sustainOn s \sustainOff s |
    \barRest |
}
% === END INCLUDE: partSeven.ly ===
% === BEGIN INCLUDE: outro.ly ===
\version "2.18.2"

outroSilent =  {
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

outroRHvI =  \relative a' {
    \once \override Slur.positions = #'(3.5 . 3.5)
    a8( g4 b8 a4) |
    g4 g'( a) |
    \once \override Slur.positions = #'(6.5 . 6)
    a8( g4 b8 a4) |
    g4 g' g |
    a,8 g4 b8 a4 |
    a,8 g4 b8 a4 |
    a,8 g4 b8 a4 |
    g4 <g bf ef g>2 \fermata |
    <g b d g>2.|
}

outroRHvII =  \relative b {
    b4( c2)( |
    b4) r r |
    b'4( c2)( |
    b4) r r |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

outroRH =  {
    <<
        \outroRHvI \\
        \outroRHvII
    >>
}


outroLHvI =  \relative d {
    \once \override Slur.positions = #'(3.8 . 3.8)
    d4( e ef |
    d) s s |
    \once \override Slur.positions = #'(6.5 . 6.5)
    d'4( e ef |
    d) s s |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
    \barRest |
}

outroLHvII =  \relative g, {
    g2. |
    g4 s s |
    g'2. |
    g4 s s |
    \stemNeutral
    \change Staff = "rh"
    <g' b d>2. |
    \change Staff = "lh"
    <g, b d> |
    <g, b d> |
    <g b d>4 <ef ef,>2^\fermata |
    <g g,>2. |
}

outroLH =  {
    <<
        <<
            \outroLHvI \\
            \outroLHvII
        >>
        {\barRest | s4 r r | \barRest | s4 r r | }
    >>
}

outroSuper =  {
    s4^\markup {Andante} s s |
    \barRest |
    \barRest |
    \barRest |
    s4^\markup {Tempo primo} s s |
    \barRest |
    \barRest |
    \barRest |
    s2.^\markup { \smaller FINE} |
}

outroDynamics =  {
    s4\p s s |
    \barRest |
    \barRest |
    \barRest |
    s2.\f |
    s2.\f |
    s2.\f |
    s2.\f |
    s2.\f |
}

outroSub =  {
    \outroSilent
}
% === END INCLUDE: outro.ly ===

playSilent =  {
                                % 1-8
    \introSilent      \bar "||" \break

                                % 9-24
    \partOneSilent    \bar "||" \break
                                % 25-28
    \segueOneSilent

                                % 29-45
    \partTwoSilent
                                % 46-52
    \segueTwoSilent   \bar "||" \break

                                % 53-68
    \partThreeSilent  \bar "||" \break
                                % 69-76
    \segueThreeSilent  \bar "||" \break

                                % 77-97
    \partFourSilent   \bar "||" \break
                                % 98- 101
    \segueFourSilent  \bar "||" \break

                                % 102-118
    \partFiveSilent

                                % 119-135
    \partSixSilent
                                % 136-140
    \segueSixSilent   \bar "||" \break

                                % 141-155
    \partSevenSilent
                                % 156-164
    \outroSilent

                                % Place a closing fermata over the final bar line
    \once \override Score.RehearsalMark.self-alignment-X = #0.5
    \once \override Score.RehearsalMark.Y-offset = #-3.5
    \once \override Score.RehearsalMark.outside-staff-priority = #100
    \mark \markup { \fermata }

    \bar "|."
}

playSuper =  {
    \introSuper
    \partOneSuper
    \segueOneSuper
    \partTwoSuper
    \segueTwoSuper
    \partThreeSuper
    \segueThreeSuper
    \partFourSuper
    \segueFourSuper
    \partFiveSuper
    \partSixSuper
    \segueSixSuper
    \partSevenSuper
    \outroSuper
}

playRH =  {
    \time 3/4
    \key g \major
    \clef treble
    \introRH
    \partOneRH
    \segueOneRH
    \key bf \major
    \partTwoRH
    \segueTwoRH
    \key g \major
    \partThreeRH
    \segueThreeRH
    \key f \major
    \partFourRH
    \segueFourRH
    \key d \major
    \partFiveRH
    \partSixRH
    \segueSixRH
    \key g \major
    \partSevenRH
    \outroRH
}

playDynamics =  {
    \introDynamics
    \partOneDynamics
    \segueOneDynamics
    \partTwoDynamics
    \segueTwoDynamics
    \partThreeDynamics
    \segueThreeDynamics
    \partFourDynamics
    \segueFourDynamics
    \partFiveDynamics
    \partSixDynamics
    \segueSixDynamics
    \partSevenDynamics
    \outroDynamics
}

playLH =  {
    \time 3/4
    \key g \major
    \clef bass
    \introLH
    \partOneLH
    \segueOneLH
    \key bf \major
    \partTwoLH
    \segueTwoLH
    \key g \major
    \partThreeLH
    \segueThreeLH
    \key f \major
    \partFourLH
    \segueFourLH
    \key d \major
    \partFiveLH
    \partSixLH
    \segueSixLH
    \key g \major
    \partSevenLH
    \outroLH
}

playSub =  {
    \introSub
    \partOneSub
    \segueOneSub
    \partTwoSub
    \segueTwoSub
    \partThreeSub
    \segueThreeSub
    \partFourSub
    \segueFourSub
    \partFiveSub
    \partSixSub
    \segueSixSub
    \partSevenSub
    \outroSub
}

scoreSuper =  {
    \new Dynamics = "super" {
        <<
            \playSilent
            \playSuper
        >>
    }
}

scoreRH =  {
    \new Staff = "rh" {
        <<
            \playSilent
%%%            \playSuper
            \playRH
        >>
    }
}

scoreDynamics =  {
    \new Dynamics = "dyn" {
        <<
            \playSilent
            \playDynamics
        >>
    }
}

scoreLH =  {
    \new Staff = "lh" {
        \set Staff.pedalSustainStyle = #'mixed
        <<
            \playSilent
            \playLH
%%%            \playSub
        >>
    }
}

scoreSub =  {
    \new Dynamics = "sub" {
        <<
            \playSilent
            \playSub
        >>
    }
}

scoreAll =  {
    \new PianoStaff {
        \set PianoStaff.midiInstrument = "honky-tonk"
        %% \set PianoStaff.followVoice = ##t
        \set PianoStaff.connectArpeggios = ##t
        %% \accidentalStyle piano
        <<
            \scoreSuper
            \scoreRH
            \scoreDynamics
            \scoreLH
            \scoreSub
        >>
    }
}

                                % PAPER ONLY, NO MIDI
\score {
    \scoreAll
    \layout { }
}



                                % ALL REPEATS, MIDI ONLY
\score {
    \unfoldRepeats \scoreAll
    \midi {
        \tempo 4=150
    }
}



%{
convert-ly (GNU LilyPond) 2.18.2  convert-ly: Processing `'...
Applying conversion: 2.3.1, 2.3.2, 2.3.4, 2.3.6, 2.3.8, 2.3.9, 2.3.10,
2.3.11, 2.3.12, 2.3.16, 2.3.17, 2.3.18, 2.3.22, 2.3.23, 2.3.24,
2.3.25, 2.4.0, 2.5.0, 2.5.1, 2.5.2, 2.5.3, 2.5.12, 2.5.13, 2.5.17,
2.5.18, 2.5.21, 2.5.25, 2.6.0, 2.7.0, 2.7.1, 2.7.2, 2.7.4, 2.7.6,
2.7.10, 2.7.11, 2.7.12, 2.7.13, 2.7.14, 2.7.15, 2.7.22, 2.7.24,
2.7.28, 2.7.29, 2.7.30, 2.7.31, 2.7.32, 2.7.32, 2.7.36, 2.7.40, 2.9.4,
2.9.6, 2.9.9, 2.9.11, 2.9.13, 2.9.16, 2.9.19, 2.10.0, 2.11.2, 2.11.3,
2.11.5, 2.11.6, 2.11.10, Span_dynamic_performer has been merged into
Dynamic_performer2.11.11, 2.11.13, 2.11.15,  Not smart enough to
convert VerticalAlignment #'forced-distance. Use the `alignment-
offsets' sub-property of NonMusicalPaperColumn #'line-break-system-
details to set fixed distances between staves. 2.11.23, 2.11.35,
2.11.38, 2.11.46, 2.11.48, 2.11.50, 2.11.51, 2.11.52, 2.11.53,
2.11.55, 2.11.57, 2.11.60, 2.11.61, 2.11.62, 2.11.64, 2.12.0, 2.12.3,
2.13.0, 2.13.1, 2.13.4, 2.13.10, 2.13.16, 2.13.18, 2.13.20, 2.13.27,
2.13.29, 2.13.31, 2.13.36, 2.13.39, 2.13.40, 2.13.42, 2.13.44,
2.13.46,  Vertical spacing changes might affect user-defined contexts.
Please refer to the manual for details, and update manually. 2.13.48,
2.13.51, 2.14.0, 2.15.7, 2.15.9, 2.15.10, 2.15.16, 2.15.17, 2.15.18,
2.15.19, 2.15.20, 2.15.25, 2.15.32, 2.15.39, 2.15.40, 2.15.42,
2.15.43, 2.16.0, 2.17.0, 2.17.4, 2.17.5, 2.17.6, 2.17.11, 2.17.14,
2.17.15, 2.17.18, 2.17.19, 2.17.20, 2.17.25, 2.17.27, 2.17.29,
2.17.97, 2.18.0
%}
