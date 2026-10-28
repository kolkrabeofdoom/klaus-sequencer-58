# Changelog

## 0.4.0 – Klaus arpeggiert

- Live-Arpeggiator mit Aufwärts, Abwärts, Auf/Ab, Zufall und Spielreihenfolge
- Latch mit explizitem Leeren
- ein bis vier Oktaven mit MIDI-Grenzprüfung
- eigene Arp-Rate, Gate und ein bis acht Arp-Ratchets
- direkte MIDI-Ausgabe über den bestehenden Scheduler
- Rendern des laufenden Arpeggios in ein neues Kettenpattern
- mehrere Pattern pro Kette, aktive Pattern-IDs und Patternwechsel im Inspector
- aktive Bearbeitungen werden vor Wechsel und Autosave ins Pattern zurückgeschrieben
- Projektformat 2 mit automatischer Migration älterer Projekte
- scrollbarer Inspector für Touch- und Kleinbildschirme

## 0.3.0 – MIDI wird konkret

- MIDI-Ein- und -Ausgänge erkennen, aktualisieren, verbinden und trennen
- eigener Ausgangs-Scheduler für Note-On, Note-Off, Gate und Ratchets
- Live-Instrument, MIDI-Thru, Akkord-Capture, Step-Record und quantisierte Aufnahme
- echte Ketten-Playheads statt parallel laufender verketteter Reihen
- vier Laufrichtungen, individuelle Teilungen und Swing pro Kette
- MIDI-Kanal, Richtung, Teilung und Swing im Inspector editierbar
- temporäre Live-Skips beeinflussen die tatsächliche Kettenlänge
- atomisches Autosave mit Wiederherstellung und Migration unvollständiger Altprojekte
- PANIC sendet CC 123 und CC 120 auf allen MIDI-Kanälen

## 0.1.0 – Erster ausführbarer Prototyp

- 8 × 16 Step-Matrix
- polyphone Melodie- und Drum-Pianorolls
- Probability, Ratchets, Active, Rest und Skip
- Edit- und Performance-Modus
- lokaler und globaler temporärer Live-Skip
- kanonischer Klaus-Charakter und Splashscreen
