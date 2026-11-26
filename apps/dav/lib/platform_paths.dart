import 'dart:io';

String get userHome =>
    Platform.environment[Platform.isWindows ? 'USERPROFILE' : 'HOME'] ??
    Directory.current.path;
String get userData => Platform.isWindows
    ? (Platform.environment['LOCALAPPDATA'] ?? '$userHome/AppData/Local')
    : (Platform.environment['XDG_DATA_HOME'] ?? '$userHome/.local/share');
String executableName(String name) => Platform.isWindows ? '$name.exe' : name;
