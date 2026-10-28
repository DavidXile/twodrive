import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/connection_access.dart';

class AccessClient extends StorageClient {
  final calls = <Json>[];
  bool rejectAccess = false;
  AccessClient({bool configured = false, bool locked = false}) {
    available = true;
    selected = 'b';
    providers = [
      {
        'id': 'fixture',
        'capabilities': {'upload': true},
        'write_policy': '覆盖前保留本机恢复副本',
      },
    ];
    profiles = [
      {
        'id': 'a',
        'revision': 'ra',
        'kind': 'fixture',
        'name': '测试网盘 A',
        'writable': locked,
        'write_locked': locked,
      },
      {'id': 'b', 'revision': 'rb', 'kind': 'fixture', 'name': '测试网盘 B'},
    ];
    status = {
      'connected': true,
      'profile': 'b',
      'write_lock': {'configured': configured},
    };
  }
  @override
  Future<void> setAccess(Json owner, String mode, String password) async {
    calls.add({
      'profile': owner['id'],
      'revision': owner['revision'],
      'mode': mode,
      'password': password,
    });
    error = rejectAccess ? '写入密码错误' : null;
  }
}

Future<void> show(WidgetTester tester, AccessClient client) async {
  await tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: ConnectionAccess(client: client, profile: client.profiles[0]),
      ),
    ),
  );
  await tester.tap(find.byKey(const ValueKey('access-a')));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
    'first editable mode requires matching local password and targets the card owner',
    (tester) async {
      final c = AccessClient();
      await show(tester, c);
      await tester.tap(find.byType(DropdownButtonFormField<String>));
      await tester.pumpAndSettle();
      await tester.tap(find.text('可编辑').last);
      await tester.pumpAndSettle();
      expect(find.text('设置本机写入密码'), findsOneWidget);
      expect(find.text('覆盖前保留本机恢复副本'), findsOneWidget);
      expect(
        tester
            .widgetList<TextField>(find.byType(TextField))
            .every((f) => f.obscureText),
        isTrue,
      );
      await tester.tap(find.text('解锁并应用'));
      await tester.pumpAndSettle();
      expect(c.calls, isEmpty);
      expect(find.textContaining('两次输入一致'), findsOneWidget);
      await tester.enterText(
        find.byType(TextField).at(0),
        'test-only-password',
      );
      await tester.enterText(
        find.byType(TextField).at(1),
        'test-only-password',
      );
      await tester.tap(find.text('解锁并应用'));
      await tester.pumpAndSettle();
      expect(c.calls.single['profile'], 'a');
      expect(c.calls.single['revision'], 'ra');
      expect(c.calls.single['mode'], 'editable');
      expect(c.selected, 'b');
      expect(find.byType(AlertDialog), findsNothing);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
  testWidgets(
    'wrong password keeps the permission dialog open and offers retry',
    (tester) async {
      final c = AccessClient(configured: true, locked: true)
        ..rejectAccess = true;
      await show(tester, c);
      expect(find.text('本机写入密码'), findsOneWidget);
      expect(find.text('确认写入密码'), findsNothing);
      await tester.enterText(find.byType(TextField), 'test-wrong-password');
      await tester.tap(find.text('解锁并应用'));
      await tester.pumpAndSettle();
      expect(find.text('写入密码错误'), findsOneWidget);
      expect(find.byType(AlertDialog), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
  testWidgets('readonly mode does not request a password', (tester) async {
    final c = AccessClient(configured: true);
    await show(tester, c);
    expect(find.byType(TextField), findsNothing);
    await tester.tap(find.text('应用'));
    await tester.pumpAndSettle();
    expect(c.calls.single['mode'], 'readonly');
    expect(c.calls.single['password'], '');
    await tester.pumpWidget(const SizedBox());
    c.dispose();
  });
}
