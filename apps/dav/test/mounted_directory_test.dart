import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/mounted_directory.dart';

final class _MountInfoOverride extends IOOverrides {
  _MountInfoOverride(this.mountInfo);

  final File mountInfo;

  @override
  File createFile(String path) =>
      path == '/proc/self/mountinfo' ? mountInfo : super.createFile(path);
}

void main() {
  test(
    'directory preview accepts both direct TwoDrive FUSE sources',
    () async {
      final root = await Directory.systemTemp.createTemp('td-mount-preview-');
      addTearDown(() => root.delete(recursive: true));
      final folder = await Directory('${root.path}/mount').create();
      await File('${folder.path}/preview.txt').create();
      final mountInfo = File('${root.path}/mountinfo');
      for (final source in ['twodrive-dev', 'twodrive', 'unrelated-fuse']) {
        await mountInfo.writeAsString(
          '42 34 0:77 / ${folder.path} rw - fuse $source rw\n',
        );
        final entries = await IOOverrides.runWithIOOverrides(
          () => readMountedDirectory(
            configuredRoot: folder.path,
            mountedPath: folder.path,
            directory: '/',
          ),
          _MountInfoOverride(mountInfo),
        );
        if (source == 'unrelated-fuse') {
          expect(entries, isNull);
        } else {
          expect(entries?.single['path'], '/preview.txt', reason: source);
        }
      }
    },
    skip: !Platform.isLinux,
  );
}
