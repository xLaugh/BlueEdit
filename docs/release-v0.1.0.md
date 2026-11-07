Première version publique de **Blue Edit**, logiciel de montage vidéo à pistes pour Windows, macOS et Linux.

## Quel fichier choisir

| Système | Fichier |
|---|---|
| Windows (recommandé) | `BlueEdit-0.1.0-Setup.exe` : installateur sans droits administrateur, ffmpeg inclus |
| Windows portable | `BlueEdit-0.1.0-windows-portable.exe` : un seul fichier ; ffmpeg doit être installé à part |
| Linux, toutes distributions | `BlueEdit-0.1.0-x86_64.AppImage` (`chmod +x`, puis lancer ; installer ffmpeg) |
| Debian / Ubuntu | `blue-edit_0.1.0_amd64.deb` : `sudo apt install ./blue-edit_0.1.0_amd64.deb` |
| Linux, archive | `blue-edit-0.1.0-linux-x86_64.tar.gz` : décompresser, puis `./install.sh` |
| macOS 11+ (Apple Silicon et Intel) | `BlueEdit-0.1.0-macos-universal.dmg` ; installer ffmpeg avec `brew install ffmpeg` |

L'application n'est signée ni par Microsoft ni par Apple : Windows SmartScreen peut afficher un avertissement
(*Informations complémentaires* > *Exécuter quand même*) et, sur macOS, il faut faire clic droit > *Ouvrir* au premier lancement.

Les versions Linux et macOS sont construites automatiquement mais n'ont pas encore été testées sur une vraie machine :
n'hésitez pas à ouvrir une [issue](https://github.com/xLaugh/BlueEdit/issues) en cas de problème.
