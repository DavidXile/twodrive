import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/local_file_preflight.dart';

class LocalClient extends StorageClient {
  Json? seen;
  Completer<Json>? deferred;
  Json reply = {
    'schema_version': 1,
    'decision': 'full_file_needs_approved_plan',
    'approval_required': true,
    'content_read': false,
    'hydration_requested': false,
    'import_performed': false,
  };
  @override
  Future<Json> request(
    String op, [
    Json args = const {},
    int? requestId,
  ]) async {
    expect(op, 'local_preflight');
    seen = args;
    return deferred?.future ?? reply;
  }
}

void main() {
  Future<void> form(WidgetTester tester, LocalClient client) =>
      tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: LocalFilePreflightCard(client: client),
            ),
          ),
        ),
      );
  testWidgets('editing a completed path clears the old result', (tester) async {
    final c = LocalClient()
      ..status = {
        'local_files': {'schema_version': 1, 'metadata_preflight': true},
      };
    await form(tester, c);
    await tester.enterText(find.byType(TextField), r'C:\generated\A');
    await tester.tap(find.text('检查文件属性'));
    await tester.pumpAndSettle();
    expect(find.textContaining('尚未确认云端上传完成'), findsOneWidget);
    await tester.enterText(find.byType(TextField), r'C:\generated\B');
    await tester.pump();
    expect(find.textContaining('尚未确认云端上传完成'), findsNothing);
    await tester.pumpWidget(const SizedBox());
    c.dispose();
  });
  testWidgets('late A response cannot overwrite an edited B request', (
    tester,
  ) async {
    final c = LocalClient()
      ..status = {
        'local_files': {'schema_version': 1, 'metadata_preflight': true},
      };
    final pendingA = Completer<Json>();
    c.deferred = pendingA;
    await form(tester, c);
    await tester.enterText(find.byType(TextField), r'C:\generated\A');
    await tester.tap(find.text('检查文件属性'));
    await tester.pump();
    await tester.enterText(find.byType(TextField), r'C:\generated\B');
    await tester.pump();
    final pendingB = Completer<Json>();
    c.deferred = pendingB;
    await tester.tap(find.text('检查文件属性'));
    await tester.pump();
    pendingA.complete(Map<String, dynamic>.from(c.reply));
    await tester.pump();
    expect(find.textContaining('尚未确认云端上传完成'), findsNothing);
    expect(find.text('检查中…'), findsOneWidget);
    pendingB.complete({...c.reply, 'decision': 'unsupported_cache'});
    await tester.pumpAndSettle();
    expect(find.textContaining('私有缓存无法视为完整'), findsOneWidget);
    expect(c.seen?['source_path'], r'C:\generated\B');
    await tester.pumpWidget(const SizedBox());
    c.dispose();
  });
  test(
    'old or future engines cannot imply native mount or preflight support',
    () async {
      final c = LocalClient();
      expect(c.canPreflightLocalFile, isFalse);
      c.status = {
        'local_files': {'schema_version': 2, 'metadata_preflight': true},
      };
      await expectLater(
        c.preflightLocalFile('generated', 'unknown'),
        throwsStateError,
      );
      expect(c.seen, isNull);
      c.status = {
        'local_files': {'schema_version': 1, 'metadata_preflight': true},
      };
      expect(c.canPreflightLocalFile, isTrue);
      expect(c.canMount, isFalse);
      c.reply['schema_version'] = 2;
      await expectLater(
        c.preflightLocalFile('generated', 'unknown'),
        throwsStateError,
      );
      c.dispose();
    },
  );
  testWidgets(
    'GUI only sends IPC and does not call a complete file synchronized',
    (tester) async {
      final c = LocalClient()
        ..status = {
          'local_files': {'schema_version': 1, 'metadata_preflight': true},
        };
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: LocalFilePreflightCard(client: c),
            ),
          ),
        ),
      );
      await tester.enterText(
        find.byType(TextField),
        r'C:\generated\fixture.bin',
      );
      await tester.tap(find.text('检查文件属性'));
      await tester.pumpAndSettle();
      expect(c.seen?['source_kind'], 'unknown');
      expect(c.seen?['schema_version'], 1);
      expect(find.textContaining('尚未确认云端上传完成'), findsOneWidget);
      expect(find.text('已同步'), findsNothing);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
}
