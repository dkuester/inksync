# -*- coding: utf-8 -*-
from __future__ import annotations

import os
from pathlib import Path

from PyQt5.Qt import QAction, QIcon
from PyQt5.QtWidgets import QFileDialog # <-- Diese Zeile ist neu!
from calibre.gui2.actions import InterfaceAction
from calibre.utils.resources import get_path as get_plugin_path
from calibre.gui2 import error_dialog, info_dialog

# Später werden Sie hier inkt.py und reading_stats.py importieren

class InksyncPlugin(InterfaceAction):

    name = 'Inksync'
    action_spec = (_('Inksync: Kobo Daten importieren'), 'inksync_icon.png',
                   _('Importiert Notizen und generiert Lesestatistiken von Ihrem Kobo E-Reader.'),
                   _('Importiert Notizen und generiert Lesestatistiken'))

    def genesis(self):
        self.qaction = QAction(self.gui)
        self.qaction.setText(_('Inksync: Kobo Daten importieren'))
        icon_path = get_plugin_path('inksync_icon.png')
        if os.path.exists(icon_path):
            self.qaction.setIcon(QIcon(icon_path))
        self.qaction.triggered.connect(self.import_kobo_data)

    def import_kobo_data(self):
        # Diese Funktion wird aufgerufen, wenn der Nutzer auf den Button klickt.

        # 1. KoboReader.sqlite über Dateidialog finden
        db_path, _ = QFileDialog.getOpenFileName(
            self.gui, # Parent-Widget ist das Hauptfenster von Calibre
            _('KoboReader.sqlite auswählen'), # Titel des Dialogfensters
            str(Path.home()), # Startverzeichnis: Das Benutzer-Home-Verzeichnis ist ein guter Standard
            _('SQLite-Datenbanken (*.sqlite *.db);;Alle Dateien (*.*)') # Filter für Dateitypen
        )

        if not db_path: # Wenn der Benutzer den Dialog abbricht oder keine Datei auswählt
            error_dialog(self.gui, _('Abgebrochen'), _('Keine KoboReader.sqlite-Datei ausgewählt.'), show=True)
            return

        if not Path(db_path).is_file():
            error_dialog(self.gui, _('Fehler'), _('Die angegebene Datei existiert nicht oder ist keine gültige Datei.'), show=True)
            return

        # Wenn hier angekommen, haben wir einen gültigen Pfad zur Datenbank
        info_dialog(self.gui, _('Erfolg'), _('Kobo-Datenbank ausgewählt:\n%s') % db_path, show=True)

        # --- Hier würde dann der Code zum Ausführen von inkt.py und reading_stats.py folgen ---
        # Zum Beispiel:
        # try:
        #     output_dir = os.path.join(str(Path.home()), 'KoboInksyncExports')
        #     os.makedirs(output_dir, exist_ok=True)
        #
        #     info_dialog(self.gui, _('Inksync'), _('Exportiere Notizen...'), show=True)
        #     # export_annotations_function(db_path, output_dir) # Ihre umstrukturierte inkt.py Funktion
        #
        #     info_dialog(self.gui, _('Inksync'), _('Generiere Lesestatistiken...'), show=True)
        #     # generate_stats_function(db_path, output_dir) # Ihre umstrukturierte reading_stats.py Funktion
        #
        #     info_dialog(self.gui, _('Inksync'), _('Vorgang abgeschlossen! Exportierte Daten finden Sie unter: ') + output_dir, show=True)
        #
        # except Exception as e:
        #     error_dialog(self.gui, _('Fehler'), _('Ein Fehler ist aufgetreten: %s') % str(e), show=True)
        # finally:
        #     self.gui.library_view.model().refresh_rows()

    def is_always_enabled(self, db):
        return True