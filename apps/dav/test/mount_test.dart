import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/main.dart';
import 'package:twodrive_dav/mount_card.dart';

class MountClient extends StorageClient {
  MountClient({bool installed = true}) {
    available = true;
    selected = 'a';
    profiles = [
      {
        'id': 'a',
        'revision': 'rev-a',
        'kind': 'example',
        'name': 'Example 网盘',
        'mount_path': r'C:\TwoDrive\Example',
      },
    ];
    status = {
      'profile': 'a',
      'connected': true,
      'mounted': false,
      'capabilities': {'mount': installed},
      'mount_support': {
        'supported': true,
        'platform': 'windows',
        'readonly': true,
        'driver_installed': installed,
      },
    };
  }
  final calls = <Json>[];
  @override
  Future<Json> request(
    String op, [
    Json args = const {},
    int? requestId,
  ]) async {
    calls.add({'op': op, ...args});
    if (op == 'mount' || op == 'unmount') {
      status = {...status, 'mounted': op == 'mount'};
    }
    return op == 'status' ? status : {};
  }
}

void main() {
  testWidgets(
    'missing driver gives installation help without requesting mount',
    (tester) async {
      final c = MountClient(installed: false);
      await tester.pumpWidget(DavApp(client: c));
      await tester.tap(find.widgetWithText(TextButton, '挂载'));
      await tester.pumpAndSettle();
      expect(find.text('安装挂载组件'), findsOneWidget);
      expect(find.text('https://winfsp.dev/rel/'), findsOneWidget);
      expect(c.calls, isEmpty);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
  testWidgets(
    'mount and unmount carry profile revision; header follows status',
    (tester) async {
      tester.view.physicalSize = const Size(840, 580);
      tester.view.devicePixelRatio = 1;
      tester.platformDispatcher.textScaleFactorTestValue = 1.4;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final c = MountClient();
      await tester.pumpWidget(DavApp(client: c));
      await tester.tap(find.widgetWithText(TextButton, '挂载'));
      await tester.pumpAndSettle();
      expect(c.calls.where((v) => v['op'] == 'mount').single, {
        'op': 'mount',
        'profile': 'a',
        'revision': 'rev-a',
      });
      expect(find.widgetWithText(TextButton, '卸载'), findsOneWidget);
      await tester.tap(find.widgetWithText(TextButton, '卸载'));
      await tester.pumpAndSettle();
      expect(c.calls.where((v) => v['op'] == 'unmount').single, {
        'op': 'unmount',
        'profile': 'a',
        'revision': 'rev-a',
      });
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
  testWidgets(
    'read-only mount card explains behavior and disables absent driver',
    (tester) async {
      final c = MountClient(installed: false);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: MountCard(client: c, onOpen: () {}),
            ),
          ),
        ),
      );
      expect(find.text('文件管理器挂载 · 只读'), findsOneWidget);
      final button = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, '挂载'),
      );
      expect(button.onPressed, isNull);
      expect(find.textContaining('不能在挂载内修改或删除'), findsOneWidget);
      expect(find.textContaining('待上传完成'), findsNothing);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
  testWidgets(
    'a retained mount can still be unmounted when its connection fails',
    (tester) async {
      final c = MountClient();
      c.status = {...c.status, 'connected': false, 'mounted': true};
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: MountCard(client: c, onOpen: () {}),
            ),
          ),
        ),
      );
      final button = tester.widget<FilledButton>(
        find.widgetWithText(FilledButton, '卸载'),
      );
      expect(button.onPressed, isNotNull);
      await tester.tap(find.widgetWithText(FilledButton, '卸载'));
      await tester.pumpAndSettle();
      expect(c.calls.where((v) => v['op'] == 'unmount').single, {
        'op': 'unmount',
        'profile': 'a',
        'revision': 'rev-a',
      });
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );
}
