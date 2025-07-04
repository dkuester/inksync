# -*- coding: utf-8 -*-
from __future__ import annotations

import os
from pathlib import Path

from PyQt5.Qt import QAction, QIcon, QInputDialog
from calibre.gui2.actions import InterfaceAction
from calibre.utils.resources import get_path as get_plugin_path
from calibre.gui2 import error_dialog, info_dialog

# Importieren Sie hier inkt.py und reading_stats.py
# Am einfachsten wäre es, diese Skripte vorerst direkt in Ihr Plugin-Verzeichnis zu kopieren.
# Später könnten Sie überlegen, wie Sie sie besser verwalten.
# Für den Anfang:
# import inkt.py (oder inkt_main_function, wenn Sie es als Modul umstrukturieren)
# import reading_stats.py (oder reading_stats_main_function)

class InksyncPlugin(InterfaceAction):

    name = 'Inksync'
    # Die Aktionen, die dieses Plugin bereitstellt
    action_spec = (_('Inksync: Kobo Daten importieren'), 'inksync_icon.png',
                   _('Importiert Notizen und generiert Lesestatistiken von Ihrem Kobo E-Reader.'),
                   _('Importiert Notizen und generiert Lesestatistiken'))

    def genesis(self):
        # Wird einmal beim Laden des Plugins aufgerufen
        self.qaction = QAction(self.gui)
        self.qaction.setText(_('Inksync: Kobo Daten importieren'))
        # Optional: Icon laden
        icon_path = get_plugin_path('inksync_icon.png') # Stellen Sie sicher, dass in einem Ordner im Plugin vorhanden
        if os.path.exists(icon_path):
            self.qaction.setIcon(QIcon(icon_path))
        self.qaction.triggered.connect(self.import_kobo_data) # Verbindet den Klick mit Ihrer Funktion

    def import_kobo_data(self):
        # Diese Funktion wird aufgerufen, wenn der Nutzer auf den Button klickt.
        db = self.gui.library_view.db # Zugriff auf die aktuelle Calibre-Bibliothek

        # --- Hier kommt die Logik für inkt.py und reading_stats.py ---

        # 1. KoboReader.sqlite finden
        # Dies ist der kniffligste Teil. Sie müssen den Pfad zur Datenbank finden.
        # Optionen:
        # a) Benutzer fragen (QFileDialog)
        # b) Automatisch auf verbundenen Kobo-Geräten suchen (komplexer, erfordert Device-Integration)
        # c) Vordefinierten Pfad in den Plugin-Einstellungen hinterlegen

        # Beispiel: Benutzer nach dem Pfad fragen (sehr einfach für den Start)
        db_path, ok = QInputDialog.getText(self.gui, 'KoboReader.sqlite Pfad',
                                            'Bitte geben Sie den Pfad zu Ihrer KoboReader.sqlite-Datei ein:')
        if not ok or not db_path:
            error_dialog(self.gui, _('Fehler'), _('Kein Pfad zur Datenbank angegeben.'), show=True)
            return

        if not Path(db_path).is_file():
            error_dialog(self.gui, _('Fehler'), _('Die angegebene Datei existiert nicht.'), show=True)
            return

        # 2. inkt.py und reading_stats.py ausführen
        # Hier müssten Sie die Logik von inkt.py und reading_stats.py aufrufen.
        # Idealerweise refaktorieren Sie diese Skripte, sodass sie als Funktionen aufgerufen werden können
        # und nicht nur als CLI-Tools.
        # Beispiel (pseudo-code):
        try:
            # Annahme: inkt.py hat eine Funktion `export_annotations(db_path, output_dir)`
            # Annahme: reading_stats.py hat eine Funktion `generate_stats(db_path, output_dir)`
            output_dir = os.path.join(str(Path.home()), 'KoboInksyncExports') # Standard-Exportverzeichnis
            os.makedirs(output_dir, exist_ok=True)

            info_dialog(self.gui, _('Inksync'), _('Exportiere Notizen...'), show=True)
            # Hier den tatsächlichen Aufruf der inkt.py Logik einfügen
            # z.B. inkt_main_function(db_path=db_path, output_dir=output_dir, config_file='...')
            # Oder besser: die Logik direkt hier im Plugin implementieren, basierend auf inkt.py
            
            info_dialog(self.gui, _('Inksync'), _('Generiere Lesestatistiken...'), show=True)
            # Hier den tatsächlichen Aufruf der reading_stats.py Logik einfügen
            # z.B. reading_stats_main_function(db_path=db_path, output_dir=output_dir)

            # 3. Ergebnisse in Calibre integrieren (Optional, aber sehr nützlich!)
            # Dies ist der komplexere Teil.
            # a) Notizen als angehängte Dateien zu Büchern hinzufügen:
            #    Sie müssten die Calibre-Datenbank (db) abfragen, um das passende Buch zu finden
            #    (z.B. über Titel und Autor, die inkt.py ja auch exportiert).
            #    Dann `db.add_file()` oder `db.add_extra_file()` verwenden.
            # b) Lesestatistiken:
            #    Die generierte Statistik-Datei könnte als separate Datei in einem "Notizen"-Ordner
            #    innerhalb der Calibre-Bibliothek gespeichert oder in einem Calibre-Viewer-Plugin angezeigt werden.
            
            info_dialog(self.gui, _('Inksync'), _('Vorgang abgeschlossen! Exportierte Daten finden Sie unter: ') + output_dir, show=True)

        except Exception as e:
            error_dialog(self.gui, _('Fehler'), _('Ein Fehler ist aufgetreten: %s') % str(e), show=True)
        finally:
            self.gui.library_view.model().refresh_rows() # Ggf. Calibre-Ansicht aktualisieren

    def is_always_enabled(self, db):
        # Das Plugin ist immer aktiv, unabhängig von der Auswahl in Calibre.
        return True