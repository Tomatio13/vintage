#define AppVersion GetEnv("VINTAGE_VERSION")
[Setup]
AppName=VINTAGE
AppVersion={#AppVersion}
DefaultDirName={autopf}\VINTAGE
OutputDir=..\..
OutputBaseFilename=VINTAGE_{#AppVersion}_x64-setup
ArchitecturesAllowed=x64compatible
[Files]
Source: "..\..\target\release\vintage-gpui.exe"; DestDir: "{app}"
[Icons]
Name: "{autoprograms}\VINTAGE"; Filename: "{app}\vintage-gpui.exe"
