# Changelog

## 0.7.0 – Klaus erinnert sich an die Aufführung

- Performance Capture für Szenenwechsel, Patternwechsel sowie lokale und globale Live-Skips
- gemeinsame 24-PPQN-Timeline mit Takt-, Beat-, Tick- und Pulsanzeige
- Aufnahmebereitschaft schaltet automatisch in den Performance-Modus
- Transportstart und Take-Ende werden als eigene Ereignisse gespeichert
- reproduzierbares Take-Playback mit stabilen Szenen-, Ketten- und Pattern-IDs
- Live-Skip-Zustände werden beim Playback exakt wiederhergestellt
- scrollbare Ereignisliste mit einzeln löschbaren Einträgen
- Take-Fortschrittsanzeige und klare Zustände für Aufnahme und Wiedergabe
- Performance-Timeline wird vollständig in der Projektdatei gespeichert
- touchfreundliches, frei skalierbares Performance-Fenster
- Pause/Fortsetzung innerhalb eines Takes und grafische Ereignisbearbeitung bleiben für den erweiterten Songmodus vorgesehen

## 0.6.2 – Klaus respektiert unabhängige Reihen

- neue Projekte starten mit acht getrennten 16-Step-Ketten statt vorab verknüpfter Reihen 1+2 und 7+8
- **MIT NÄCHSTER REIHE VERKETTEN** direkt im Ketten-Inspector
- **KETTE TRENNEN** zerlegt die gewählte Kette wieder in unabhängige 16-Step-Reihen
- Verkettung bleibt strikt auf benachbarte Reihen desselben Typs begrenzt
- bestehende Projektdateien behalten ihre vorhandenen Verkettungen
- nicht betroffene Patterns und Szenenzuordnungen bleiben bei Strukturänderungen erhalten
- betroffene Ketten erhalten aus ihrem aktuellen musikalischen Zustand ein frisches `PATTERN 01`
- Transport und hängende MIDI-Noten werden vor Strukturänderungen sicher gestoppt

## 0.6.1 – Klaus verteilt Klangadressen

- Sound-Ziel pro Kette mit Bank Select MSB (`CC 0`), Bank Select LSB (`CC 32`) und Program Change
- MSB und LSB einzeln aktivierbar für unterschiedliche Herstellerkonventionen
- lesbare Programmnummern 1–128 mit korrekter MIDI-Übersetzung auf 0–127
- manueller Taster **SOUND JETZT SENDEN** im Ketten-Inspector
- optionales automatisches Senden aller aktivierten Sound-Ziele nach dem Verbinden des MIDI-Ausgangs
- Sound-Ziele werden in Projektdateien gespeichert; ältere Projekte erhalten sichere, deaktivierte Standardwerte
- Projektformat 5 mit automatischer Migration

## 0.6.0 – Klaus würfelt kontrolliert

- kreativer, Berlin-School-inspirierter Generator für die jeweils gewählte Reihe
- elf Tonleitern, frei wählbares Tonzentrum und begrenzbarer MIDI-Tonumfang
- acht musikalische Rollen von Bass-Puls und Ostinato bis Drum-Puls und Percussion-Satellit
- reproduzierbare Seeds: dieselben Einstellungen erzeugen dasselbe Motiv
- getrennte Regler für Motivlänge, effektive Länge, Dichte, Ordnung, Bewegung, Hitze, Drift und Mutation
- Vorschau mit explizitem Übernehmen oder Verwerfen statt ungefragter Projektmutation
- Varianten und anteilige Mutation bestehender Reihen
- Sperren für Noten, Rhythmus und Ausdruck
- rollenabhängige Polyphonie, Ratchets, Probability, Velocity und Gates
- Generatorplan für alle acht Reihen im Projektformat 4; szenenweite Erzeugung damit vorbereitet
- Piano-Roll passt seinen Tonumfang an Generatorprofil und vorhandene Noten an
- automatische Migration älterer Projektdateien

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
