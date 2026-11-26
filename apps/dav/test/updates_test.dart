import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/updates.dart';

Map<String, dynamic> release(
  String tag, {
  bool draft = false,
  bool ready = true,
}) => {
  'tag_name': tag,
  'draft': draft,
  'prerelease': true,
  'html_url': 'https://untrusted.invalid/never-open-this',
  'assets': [
    {
      'name':
          'twodrive-development-${tag.substring(tag.startsWith('dev-v') ? 5 : 1)}-windows-x64.zip',
      'size': 123,
      'state': ready ? 'uploaded' : 'new',
    },
  ],
};

void main() {
  test('only ready development versions are compared numerically', () {
    final found = DevelopmentRelease.newest([
      release('v0.2.14'),
      release('dev-v0.1.9'),
      release('dev-v0.1.10'),
      release('dev-v9.0.0', draft: true),
      release('dev-v8.0.0', ready: false),
      release('dev-v999999999999999999999.1.0'),
      release('dev-v1.2.3/evil'),
    ], 'windows-x64.zip');
    expect(found!.version, '0.1.10');
    expect(
      found.url,
      'https://github.com/LumiaBlack51/twodrive/releases/tag/dev-v0.1.10',
    );
    expect(DevelopmentRelease.compare('0.2.0', '0.1.10'), greaterThan(0));
  });

  test(
    'startup checks, manual override, restart throttling and opt-out persist',
    () async {
      final dir = await Directory.systemTemp.createTemp('twodrive-updates-');
      addTearDown(() => dir.delete(recursive: true));
      final file = File('${dir.path}/updates.json');
      var calls = 0;
      var now = DateTime.utc(2026, 10, 15);
      Future<List<dynamic>> fetch(int page) async {
        calls++;
        return [release('dev-v0.1.5')];
      }

      final first = UpdateChecker(
        fetch: fetch,
        now: () => now,
        platform: 'windows-x64.zip',
      );
      await first.start('0.1.4', file);
      expect(first.available, isTrue);
      expect(calls, 1);
      await first.check();
      expect(calls, 1);
      await first.check(manual: true);
      expect(calls, 2);
      first.dispose();
      final next = UpdateChecker(
        fetch: fetch,
        now: () => now,
        platform: 'windows-x64.zip',
      );
      addTearDown(next.dispose);
      await next.start('0.1.5', file);
      expect(calls, 2);
      now = now.add(const Duration(hours: 25));
      await next.check();
      expect(calls, 3);
      expect(next.available, isFalse);
      await next.setEnabled(false);
      now = now.add(const Duration(days: 2));
      await next.check();
      expect(calls, 3);
      expect(jsonDecode(await file.readAsString())['enabled'], isFalse);
      await next.check(manual: true);
      expect(calls, 4);
      final disabled = UpdateChecker(fetch: fetch, now: () => now);
      addTearDown(disabled.dispose);
      await disabled.start('0.1.4', file);
      expect(disabled.enabled, isFalse);
      expect(calls, 4);
    },
  );

  test(
    'slow check is single flight and disposal ignores its completion',
    () async {
      final dir = await Directory.systemTemp.createTemp('twodrive-updates-');
      addTearDown(() => dir.delete(recursive: true));
      final pending = Completer<List<dynamic>>();
      var calls = 0;
      final checker = UpdateChecker(
        fetch: (_) {
          calls++;
          return pending.future;
        },
      );
      final start = checker.start('0.1.4', File('${dir.path}/settings'));
      while (calls == 0) {
        await Future<void>.delayed(const Duration(milliseconds: 1));
      }
      final work = checker.check(manual: true);
      checker.dispose();
      pending.complete([release('dev-v0.1.5')]);
      await Future.wait([start, work]);
      expect(calls, 1);
      expect(checker.latest, isNull);
    },
  );

  test('failure is not up-to-date and MSIX never queries GitHub', () async {
    final dir = await Directory.systemTemp.createTemp('twodrive-updates-');
    addTearDown(() => dir.delete(recursive: true));
    var calls = 0;
    Future<List<dynamic>> fetch(int page) async {
      calls++;
      throw const SocketException('offline');
    }

    final checker = UpdateChecker(fetch: fetch);
    addTearDown(checker.dispose);
    await checker.start('0.1.4', File('${dir.path}/settings'));
    expect(checker.error, isNotNull);
    expect(checker.lastSuccess, isNull);
    final managed = UpdateChecker(fetch: fetch);
    addTearDown(managed.dispose);
    await managed.start('0.1.4', File('${dir.path}/managed'), managed: true);
    await managed.check(manual: true);
    expect(calls, 1);
  });

  test(
    'finds development after a full page of unrelated stable releases',
    () async {
      final dir = await Directory.systemTemp.createTemp('twodrive-updates-');
      addTearDown(() => dir.delete(recursive: true));
      final checker = UpdateChecker(
        platform: 'windows-x64.zip',
        fetch: (page) async => page == 1
            ? List.generate(100, (_) => release('v1.0.0'))
            : [release('dev-v0.1.6')],
      );
      addTearDown(checker.dispose);
      await checker.start('0.1.4', File('${dir.path}/settings'));
      expect(checker.latest!.version, '0.1.6');
    },
  );
}
