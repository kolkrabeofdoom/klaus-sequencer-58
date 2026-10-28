# Changelog

## 0.5.1 – Klaus räumt den Inspector auf

- Statusspruch im Inspector aus dem überlagernden Bottom-up-Layout gelöst
- „Klaus hat die Clock“ erscheint nun als normaler, mitscrollender Abschluss
- anfängliche Piano-Roll-Höhe für kompakte Fenster auf 220 Pixel reduziert
- mehr Platz für Matrix, achte Drumreihe und Arpeggiator-Inspector

## 0.5.0 – Klaus synchronisiert die Umlaufbahnen

- Szenen speichern die aktive Patternauswahl aller Ketten gemeinsam
- Szenen anlegen, umbenennen, aktualisieren und live vormerken
- quantisierte Szenenwechsel: sofort, nächster Step, Beat oder Takt
- gemeinsame 24-PPQN-Transportzeit für unabhängige Ketten
- MIDI-Clock als Internal, Master und Follower
- MIDI Start, Continue und Stop als echte einbyteige Realtime-Nachrichten
- geglättete Tempoerkennung und Clock-Timeout-Anzeige im Follower-Modus
- Öffnen, Speichern und Speichern unter mit nativen Dateidialogen
- Undo/Redo mit bis zu 64 vollständigen Projekt-Snapshots
- Performance-Timeline im Datenmodell vorbereitet
- Projektformat 3 mit automatischer Migration älterer Projekte
- GitHub Actions für Formatierung, Tests, Clippy und Windows-Release-Builds

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
