import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/main.dart';
import 'package:twodrive_dav/app_theme.dart';
import 'package:twodrive_dav/profile_dialog.dart';
import 'package:twodrive_dav/tray_panel.dart';
import 'ui_preview.dart';

StorageClient connection({
  bool adapterWrites = false,
  bool configuredWrites = false,
  Json capabilities = const {},
}) => StorageClient()
  ..available = true
  ..selected = 'example'
  ..providers = [
    {
      'id': 'example',
      'label': 'Example 网盘',
      'fields': <Json>[],
      'capabilities': {
        'upload': adapterWrites,
        'conditional_upload': adapterWrites,
        'mkdir': adapterWrites,
        'rename': adapterWrites,
        'delete': adapterWrites,
      },
    },
  ]
  ..profiles = [
    {
      'id': 'example',
      'kind': 'example',
      'name': 'Example 网盘',
      'provider_label': 'Example 网盘',
      'writable': configuredWrites,
    },
  ]
  ..status = {
    'connected': true,
    'profile': 'example',
    'capabilities': capabilities,
  };

void main() {
  setUpAll(loadPreviewFonts);
  testWidgets(
    'Windows text roles share a Chinese and Latin font family',
    (tester) async {
      final c = connection();
      await tester.pumpWidget(DavApp(client: c));
      final theme = Theme.of(tester.element(find.text('TwoDrive')));
      for (final style in [
        theme.textTheme.headlineLarge,
        theme.textTheme.titleMedium,
        theme.textTheme.bodyMedium,
        theme.textTheme.labelLarge,
      ]) {
        expect(style!.fontFamily, 'Microsoft YaHei UI');
        expect(style.fontFamilyFallback, contains('Microsoft YaHei'));
      }
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
    variant: TargetPlatformVariant({TargetPlatform.windows}),
  );

  testWidgets(
    'read-only adapter explains its limit without offering write access',
    (tester) async {
      final c = connection();
      final before = Map<String, dynamic>.from(c.status);
      await tester.pumpWidget(DavApp(client: c));
      await tester.tap(find.byKey(const ValueKey('connection-access')));
      await tester.pumpAndSettle();
      expect(find.text('连接权限'), findsOneWidget);
      expect(find.textContaining('当前网盘适配器仅支持浏览和下载'), findsOneWidget);
      expect(find.text('浏览文件'), findsOneWidget);
      expect(find.text('下载文件'), findsOneWidget);
      expect(find.text('上传文件'), findsNothing);
      expect(find.byType(Switch), findsNothing);
      await tester.tap(find.text('知道了'));
      await tester.pumpAndSettle();
      expect(find.byType(AlertDialog), findsNothing);
      expect(c.status, before);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );

  testWidgets(
    'read-only configuration explains where to enable supported writes',
    (tester) async {
      final c = connection(adapterWrites: true);
      await tester.pumpWidget(DavApp(client: c));
      await tester.tap(find.byKey(const ValueKey('connection-access')));
      await tester.pumpAndSettle();
      expect(find.text('客户端和挂载权限'), findsOneWidget);
      expect(find.textContaining('客户端和文件管理器挂载共用权限'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );

  testWidgets(
    'negotiated permissions list only enabled operations at large text size',
    (tester) async {
      tester.platformDispatcher.textScaleFactorTestValue = 1.4;
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      tester.view.physicalSize = const Size(840, 580);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final c = connection(
        adapterWrites: true,
        configuredWrites: true,
        capabilities: {
          'upload': true,
          'conditional_upload': false,
          'rename': true,
        },
      );
      await tester.pumpWidget(DavApp(client: c));
      final button = find.byKey(const ValueKey('connection-access'));
      expect(
        find.descendant(of: button, matching: find.text('可编辑')),
        findsOneWidget,
      );
      await tester.tap(button);
      await tester.pumpAndSettle();
      expect(find.text('重命名'), findsOneWidget);
      expect(find.text('上传文件'), findsNothing);
      expect(find.text('删除'), findsNothing);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      c.status['capabilities'] = {};
      await tester.pumpWidget(DavApp(client: c));
      await tester.tap(find.byKey(const ValueKey('connection-access')));
      await tester.pumpAndSettle();
      expect(find.text('客户端和挂载权限'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
  );

  testWidgets(
    'shared typography covers connections, form, permissions and tray',
    (tester) async {
      tester.view.physicalSize = const Size(1200, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      final c = connection();
      c.profiles[0]['name'] = 'iCloud Drive';
      c.profiles[0]['provider_label'] = 'iCloud Drive';
      c.profiles.add({
        'id': 'other',
        'kind': 'example',
        'name': 'OneDrive',
        'provider_label': 'OneDrive',
        'account_name': '示例账号 Example',
      });
      c.status.addAll({
        'multi_connection': true,
        'connections': [
          {'id': 'example'},
          {'id': 'other'},
        ],
      });
      for (final p in c.profiles) {
        c.quotas[p['id']] = {
          'state': 'ready',
          'usage': {
            'total': 10737418240,
            'used': 4294967296,
            'free': 6442450944,
          },
        };
      }
      await tester.pumpWidget(
        RepaintBoundary(
          key: const ValueKey('appearance-preview'),
          child: DavApp(client: c),
        ),
      );
      await tester.tap(find.text('连接').first);
      await tester.pumpAndSettle();
      await appearanceSnapshot(tester, 'ui-font-connections');
      await tester.tap(find.byKey(const ValueKey('connection-access')));
      await tester.pumpAndSettle();
      final permissionTheme = Theme.of(tester.element(find.text('连接权限')));
      expect(
        permissionTheme.textTheme.bodyMedium!.fontFamily,
        'Microsoft YaHei UI',
      );
      await appearanceSnapshot(tester, 'ui-permissions');
      await tester.tap(find.text('知道了'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('添加连接'));
      await tester.pumpAndSettle();
      expect(find.byType(ProfileDialog), findsOneWidget);
      final formTheme = Theme.of(tester.element(find.byType(ProfileDialog)));
      expect(formTheme.textTheme.bodyLarge!.fontFamily, 'Microsoft YaHei UI');
      await appearanceSnapshot(tester, 'ui-font-form');
      await tester.pumpWidget(const SizedBox());
      tester.view.physicalSize = const Size(390, 600);
      await tester.pumpWidget(
        RepaintBoundary(
          key: const ValueKey('appearance-preview'),
          child: MaterialApp(
            debugShowCheckedModeBanner: false,
            theme: appTheme(),
            home: TrayPanel(
              client: c,
              onManage: ([int page = 0]) async {},
              onOpenFolder: () async {},
              onQuit: () async {},
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      await appearanceSnapshot(tester, 'ui-font-tray');
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      c.dispose();
    },
    variant: TargetPlatformVariant({TargetPlatform.windows}),
  );
}
