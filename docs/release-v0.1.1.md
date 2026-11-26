## Nouveautés

- **Copier, couper, coller** : Ctrl+C, Ctrl+X et Ctrl+V (ainsi que Ctrl+Inser, Maj+Suppr et Maj+Inser) fonctionnent dans la ligne de temps.
  Le collage se fait sur la piste active, les autres pistes copiées gardent leur écart, et chaque événement va sur une piste de son type.
- **Clic droit sur un événement** : Couper, Copier, Supprimer et Coller, avec leurs raccourcis.
- **Sélection au rectangle** : glisser dans une zone vide de la ligne de temps sélectionne les événements touchés et la plage de temps.
- **Pistes** : clic sur le numéro d'une piste pour la sélectionner (Ctrl+clic pour en ajouter, Maj+clic pour une plage), Suppr pour supprimer
  les pistes sélectionnées (Ctrl+Z pour les remettre). Clic droit dans la colonne des pistes, sous la dernière : *Insérer une piste vidéo* ou *audio*.
- **Préférence** « Vider le presse-papiers à la fermeture du projet » (activée par défaut). Le presse-papiers du système n'est vidé
  que s'il contient encore ce que Blue Edit y a mis.

## Quel fichier choisir

| Système | Fichier |
|---|---|
| Windows (recommandé) | `BlueEdit-0.1.1-Setup.exe` : installateur sans droits administrateur, ffmpeg inclus |
| Windows portable | `BlueEdit-0.1.1-windows-portable.exe` : un seul fichier ; ffmpeg doit être installé à part |
| Linux, toutes distributions | `BlueEdit-0.1.1-x86_64.AppImage` (`chmod +x`, puis lancer ; installer ffmpeg) |
| Debian / Ubuntu | `blue-edit_0.1.1_amd64.deb` : `sudo apt install ./blue-edit_0.1.1_amd64.deb` |
| Linux, archive | `blue-edit-0.1.1-linux-x86_64.tar.gz` : décompresser, puis `./install.sh` |
| macOS 11+ (Apple Silicon et Intel) | `BlueEdit-0.1.1-macos-universal.dmg` ; installer ffmpeg avec `brew install ffmpeg` |

L'installateur Windows met à jour la version 0.1.0 déjà installée.
