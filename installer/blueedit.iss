; Installateur de Blue Edit (Inno Setup 6) : installation pour l'utilisateur, sans droits administrateur.
;   iscc /DAppVersion=0.1.0 /DExe=..\dist\windows\blue.exe [/DFfmpegDir=..\dist\windows\ffmpeg] installer\blueedit.iss
;   →   dist\windows\BlueEdit-<version>-Setup.exe
; Avec FfmpegDir, ffmpeg.exe et ffprobe.exe sont installés à côté du programme (import et export vidéo).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\target\release\blue.exe"
#endif

[Setup]
AppId={{B7E2D4A1-3C5F-4E8B-9A16-5D0F2C7B8E43}
AppName=Blue Edit
AppVersion={#AppVersion}
AppVerName=Blue Edit {#AppVersion}
AppPublisher=xLaugh
AppPublisherURL=https://github.com/xLaugh/BlueEdit
AppSupportURL=https://github.com/xLaugh/BlueEdit/issues
AppUpdatesURL=https://github.com/xLaugh/BlueEdit/releases
VersionInfoVersion={#AppVersion}
VersionInfoCompany=xLaugh
VersionInfoDescription=Installation de Blue Edit
DefaultDirName={localappdata}\Programs\Blue Edit
DisableDirPage=auto
DisableProgramGroupPage=yes
DefaultGroupName=Blue Edit
PrivilegesRequired=lowest
OutputDir=..\dist\windows
OutputBaseFilename=BlueEdit-{#AppVersion}-Setup
SetupIconFile=..\assets\blue-edit.ico
UninstallDisplayIcon={app}\Blue Edit.exe
UninstallDisplayName=Blue Edit
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
fr.DesktopIcon=Créer un raccourci sur le Bureau
en.DesktopIcon=Create a desktop shortcut
fr.Shortcuts=Raccourcis :
en.Shortcuts=Shortcuts:
fr.Launch=Lancer Blue Edit
en.Launch=Launch Blue Edit
fr.Comment=Montage vidéo à pistes
en.Comment=Track-based video editor
fr.DeleteData=Supprimer aussi les réglages, plugins et modèles de Blue Edit ?
en.DeleteData=Also delete Blue Edit settings, plug-ins and models?
fr.VideosSafe=Vos vidéos et vos projets ne sont pas touchés.
en.VideosSafe=Your videos and projects are not touched.

[Tasks]
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; GroupDescription: "{cm:Shortcuts}"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "Blue Edit.exe"; Flags: ignoreversion
#ifdef FfmpegDir
Source: "{#FfmpegDir}\ffmpeg.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#FfmpegDir}\ffprobe.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#FfmpegDir}\LICENSE-ffmpeg.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
#endif

[Icons]
Name: "{autoprograms}\Blue Edit"; Filename: "{app}\Blue Edit.exe"; Comment: "{cm:Comment}"
Name: "{autodesktop}\Blue Edit"; Filename: "{app}\Blue Edit.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Blue Edit.exe"; Description: "{cm:Launch}"; Flags: nowait postinstall skipifsilent

[Code]
// Avant de désinstaller : fermer Blue Edit (sinon ses fichiers restent).
function InitializeUninstall(): Boolean;
var
  Code: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM "Blue Edit.exe"', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(500);
  Result := True;
end;

// À la désinstallation : proposer de supprimer aussi les réglages, plugins et modèles.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if (CurUninstallStep = usPostUninstall) and (not UninstallSilent) then
  begin
    Data := ExpandConstant('{localappdata}\Blue Edit');
    if DirExists(Data) then
      if MsgBox(CustomMessage('DeleteData') + #13#10 + '(' + Data + ')' + #13#10#13#10 +
                CustomMessage('VideosSafe'), mbConfirmation, MB_YESNO) = IDYES then
        DelTree(Data, True, True, True);
  end;
end;
