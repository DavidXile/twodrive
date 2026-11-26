import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/app_theme.dart';
import 'package:twodrive_dav/connection_card.dart';

void main() {
  setUpAll(() async {
    await (FontLoader('TwoDriveSans')
          ..addFont(rootBundle.load('assets/fonts/Roboto-Regular.ttf'))
          ..addFont(rootBundle.load('assets/fonts/Roboto-Bold.ttf')))
        .load();
    await (FontLoader(
      'MaterialIcons',
    )..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    await (FontLoader('TwoDriveCJK')
          ..addFont(rootBundle.load('assets/fonts/NotoSansCJKSC-Regular.otf')))
        .load();
  });
  for (final width in [420.0, 820.0]) {
    for (final scale in [1.0, 1.5, 2.0]) {
      testWidgets(
        'connection cards keep equal size and aligned actions at $width / $scale',
        (tester) async {
          tester.view.physicalSize = Size(width * 2 + 48, 1100);
          tester.view.devicePixelRatio = 1;
          tester.platformDispatcher.textScaleFactorTestValue = scale;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
          await tester.pumpWidget(
            RepaintBoundary(
              key: const ValueKey('appearance-preview'),
              child: MaterialApp(
                debugShowCheckedModeBanner: false,
                theme: appTheme(platform: TargetPlatform.linux),
                home: Scaffold(
                  body: SingleChildScrollView(
                    child: Padding(
                      padding: const EdgeInsets.all(16),
                      child: Wrap(
                        spacing: 16,
                        runSpacing: 16,
                        children: [
                          for (var i = 0; i < 4; i++)
                            SizedBox(
                              width: width,
                              child: ConnectionCard(
                                key: ValueKey('sized-card-$i'),
                                profile: {
                                  'id': 'test-$i',
                                  'kind': [
                                    'onedrive',
                                    'icloud',
                                    'baidu',
                                    'google',
                                  ][i],
                                  'name': i == 0
                                      ? 'OneDrive · demonstration.account.with.a.long.name@example.invalid'
                                      : [
                                          'OneDrive',
                                          'iCloud Drive',
                                          '百度网盘',
                                          'Google Drive',
                                        ][i],
                                  'account_name': i == 2
                                      ? '较长的示例账号名称，仅用于界面回归测试'
                                      : '示例账号',
                                  'can_reauthenticate': true,
                                },
                                online: i != 3,
                                selected: i == 0,
                                connectLabel: i == 3 ? '登录并使用' : '查看文件',
                                usage: i == 3
                                    ? const {'state': 'unsupported'}
                                    : {
                                        'state': 'ready',
                                        'usage': {
                                          'used': i == 1
                                              ? 3200000000000
                                              : 17000000000,
                                          'total': 14000000000000,
                                          'free': 13983000000000,
                                        },
                                      },
                                mountSupported: true,
                                mounted: i != 3,
                                mountPath: i == 0
                                    ? '/synthetic/TwoDrive/long folder/longer directory/OneDrive'
                                    : '/synthetic/TwoDrive/example-$i',
                                onSelect: () {},
                                onReauthenticate: () {},
                                onMount: i == 3 ? null : () {},
                                onOpenMount: i == 3 ? null : () {},
                                onDisconnect: i == 3 ? null : () {},
                                onEdit: null,
                                onDelete: null,
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();
          final sizes = [
            for (var i = 0; i < 4; i++)
              tester.getSize(find.byKey(ValueKey('sized-card-$i'))),
          ];
          expect(
            sizes.toSet(),
            hasLength(1),
            reason:
                'Content and selected status must not change the grid cell size.',
          );
          final views = <Rect>[], mounts = <Rect>[], opens = <Rect>[];
          final reauthenticate = <Rect>[];
          for (var i = 0; i < 4; i++) {
            final card = find.byKey(ValueKey('sized-card-$i'));
            final origin = tester.getTopLeft(card);
            Rect button(String label) => tester
                .getRect(
                  find.descendant(
                    of: card,
                    matching: find.widgetWithText(OutlinedButton, label),
                  ),
                )
                .shift(-origin);
            views.add(button(i == 3 ? '登录并使用' : '查看文件'));
            mounts.add(button(i == 3 ? '挂载' : '卸载'));
            opens.add(button('打开挂载'));
            reauthenticate.add(button('重新登录'));
          }
          expect(views.toSet(), hasLength(1));
          expect(mounts.toSet(), hasLength(1));
          expect(opens.toSet(), hasLength(1));
          expect(reauthenticate.toSet(), hasLength(1));
          expect(opens.first.top, greaterThan(views.first.bottom));
          expect(opens.first.left, views.first.left);
          expect(reauthenticate.first.top, opens.first.top);
          expect(reauthenticate.first.left, mounts.first.left);
          expect(
            [
              views.first,
              mounts.first,
              opens.first,
              reauthenticate.first,
            ].map((rect) => rect.size).toSet(),
            hasLength(1),
          );
          expect(tester.takeException(), isNull);
        },
      );
    }
  }

  testWidgets(
    'the card header menu keeps account actions and their availability',
    (tester) async {
      final actions = <String>[];
      Future<void> showCard({required bool online}) => tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: ConnectionCard(
              profile: const {'kind': 'google', 'name': 'Google Drive'},
              online: online,
              selected: online,
              connectLabel: online ? '查看文件' : '登录并使用',
              onSelect: () => actions.add('select'),
              onDisconnect: () => actions.add('disconnect'),
              onEdit: online ? null : () => actions.add('edit'),
              onDelete: online ? null : () => actions.add('delete'),
            ),
          ),
        ),
      );
      Future<void> openMenu() async {
        await tester.tap(find.byTooltip('更多操作'));
        await tester.pumpAndSettle();
      }

      bool enabled(String label) => tester
          .widget<PopupMenuItem<String>>(
            find.widgetWithText(PopupMenuItem<String>, label),
          )
          .enabled;

      await showCard(online: true);
      expect(find.byTooltip('断开此连接'), findsNothing);
      await openMenu();
      expect(enabled('断开此连接'), isTrue);
      expect(enabled('编辑'), isFalse);
      expect(enabled('删除配置'), isFalse);
      await tester.tap(find.text('断开此连接'));
      await tester.pumpAndSettle();

      await showCard(online: false);
      await openMenu();
      expect(enabled('断开此连接'), isFalse);
      expect(enabled('编辑'), isTrue);
      expect(enabled('删除配置'), isTrue);
      await tester.tap(find.text('编辑'));
      await tester.pumpAndSettle();
      await openMenu();
      await tester.tap(find.text('删除配置'));
      await tester.pumpAndSettle();
      expect(actions, ['disconnect', 'edit', 'delete']);
    },
  );
}
