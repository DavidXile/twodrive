import 'dart:ui' show BoxHeightStyle;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/language.dart';
import 'package:twodrive_dav/main.dart';
import 'package:twodrive_dav/workspace_navigation.dart';
import 'package:twodrive_dav/workspace_style.dart';

import 'overview_test.dart' show OverviewClient;
import 'ui_preview.dart';

class WorkspaceLayoutClient extends OverviewClient {
  WorkspaceLayoutClient() {
    status['version'] = '0.4.0';
    status['mount_support'] = {'supported': true, 'driver_installed': true};
    status['capabilities'] = {
      'mount': true,
      'upload': true,
      'manual_upload': true,
      'mkdir': true,
      'backup': true,
    };
    profiles.first['capabilities'] = {'backup': true, 'versioned_backup': true};
    path = '/Documents/Project archive/Long directory name';
    entries = fixtureEntries;
  }

  static const fixtureEntries = <Json>[
    {'path': '/Design files', 'name': 'Design files', 'directory': true},
    {'path': '/Report.pdf', 'name': 'Report.pdf', 'size': 2048},
  ];

  @override
  Future<Json> request(
    String op, [
    Json args = const {},
    int? requestId,
  ]) async {
    return switch (op) {
      'list' => {'entries': fixtureEntries},
      'backups' => {'folders': [], 'strategy': 'replace'},
      _ => super.request(op, args, requestId),
    };
  }
}

void main() {
  setUpAll(() async {
    for (final (family, fonts) in [
      ('TwoDriveSans', ['Roboto-Regular.ttf', 'Roboto-Bold.ttf']),
      ('TwoDriveCJK', ['NotoSansCJKSC-Regular.otf', 'NotoSansCJKSC-Bold.otf']),
    ]) {
      final loader = FontLoader(family);
      for (final font in fonts) {
        loader.addFont(rootBundle.load('assets/fonts/$font'));
      }
      await loader.load();
    }
    await loadPreviewFonts();
  });

  for (final language in ['zh', 'en']) {
    for (final (size, pixelRatio, scale) in [
      (const Size(1920, 1024), 1.0, 1.5),
      (const Size(1920, 1080), 1.5, 1.25),
      (const Size(840, 580), 1.0, 2.0),
    ]) {
      testWidgets(
        'workspace pages fit in $language at ${size.width} / $pixelRatio / $scale',
        (tester) async {
          tester.view.physicalSize = size;
          tester.view.devicePixelRatio = pixelRatio;
          addTearDown(tester.view.resetPhysicalSize);
          addTearDown(tester.view.resetDevicePixelRatio);
          desktopLocale.value = language;
          addTearDown(() => desktopLocale.value = 'zh');
          final client = WorkspaceLayoutClient();
          addTearDown(client.dispose);
          await tester.pumpWidget(
            RepaintBoundary(
              key: const ValueKey('appearance-preview'),
              child: Builder(
                builder: (context) => MediaQuery(
                  data: MediaQueryData.fromView(
                    View.of(context),
                  ).copyWith(textScaler: TextScaler.linear(scale)),
                  child: DavApp(client: client),
                ),
              ),
            ),
          );
          await tester.pumpAndSettle();

          void expectOneLine(Finder finder) {
            final text = tester.renderObject<RenderParagraph>(finder);
            final boxes = text.getBoxesForSelection(
              TextSelection(
                baseOffset: 0,
                extentOffset: text.text.toPlainText().length,
              ),
              boxHeightStyle: BoxHeightStyle.max,
            );
            expect(
              text.didExceedMaxLines,
              isFalse,
              reason: text.text.toPlainText(),
            );
            expect(
              boxes.map((box) => box.bottom).toSet(),
              hasLength(1),
              reason: text.text.toPlainText(),
            );
          }

          expectOneLine(find.text('Development · 0.4.0'));
          expectOneLine(find.text(tr('● {arg0} 个账号已连接', [2])));
          final navigation = find.byKey(const ValueKey('navigation'));
          void expectWhiteSelector(String key) {
            final field = find.byKey(ValueKey(key));
            expect(field, findsOneWidget);
            expect(
              find.ancestor(
                of: field,
                matching: find.byWidgetPredicate(
                  (widget) => widget is Surface && widget.color == Colors.white,
                ),
              ),
              findsOneWidget,
            );
          }

          for (final destination in WorkspacePage.sidebar) {
            final label = find.descendant(
              of: navigation,
              matching: find.text(destination.label),
            );
            await tester.scrollUntilVisible(
              label,
              70,
              scrollable: find.descendant(
                of: navigation,
                matching: find.byType(Scrollable),
              ),
            );
            expectOneLine(label);
            await tester.tap(label);
            await tester.pumpAndSettle();
            expect(tester.takeException(), isNull, reason: destination.label);
            expect(find.byKey(const ValueKey('mounted-folders')), findsNothing);
            switch (destination) {
              case WorkspacePage.files:
                expectWhiteSelector('files-target');
                expect(find.text(tr('筛选当前目录')), findsOneWidget);
              case WorkspacePage.backup:
                expectWhiteSelector('backup-target');
                expectWhiteSelector('backup-strategy');
                final help = find.text(tr('备份任务按网盘分别保存。切换网盘不会迁移已有任务。'));
                final mode = find.text(tr('备份方式'));
                expect(
                  tester.getBottomLeft(help).dy,
                  lessThan(tester.getTopLeft(mode).dy),
                );
              case WorkspacePage.settings:
                expectWhiteSelector('workspace-language');
              default:
                break;
            }
            if (pixelRatio == 1 && size.width == 1920) {
              await appearanceSnapshot(
                tester,
                'workspace-${destination.name}-$language',
              );
            }
          }
          await tester.pumpWidget(const SizedBox());
        },
        variant: TargetPlatformVariant({TargetPlatform.linux}),
      );
    }

    testWidgets(
      'Dashboard management links align in $language',
      (tester) async {
        tester.view.physicalSize = const Size(1920, 1024);
        tester.view.devicePixelRatio = 1;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        desktopLocale.value = language;
        addTearDown(() => desktopLocale.value = 'zh');
        final client = OverviewClient();
        addTearDown(client.dispose);
        await tester.pumpWidget(
          Builder(
            builder: (context) => MediaQuery(
              data: MediaQueryData.fromView(
                View.of(context),
              ).copyWith(textScaler: const TextScaler.linear(1.5)),
              child: DavApp(client: client),
            ),
          ),
        );
        Finder summaryCard(String label) => find.ancestor(
          of: find.text(tr(label)),
          matching: find.byWidgetPredicate(
            (widget) =>
                widget is Container && widget.constraints?.minHeight == 236,
          ),
        );
        final status = tester.getRect(summaryCard('同步状态'));
        final upload = tester.getRect(summaryCard('上传'));
        final download = tester.getRect(summaryCard('下载'));
        expect(upload.top, status.top);
        expect(download.top, status.top);
        expect(upload.bottom, status.bottom);
        expect(download.bottom, status.bottom);
        final mounts = find.text(tr('管理挂载 ›'));
        final connections = find.text('${tr('管理连接')} ›');
        expect(
          tester.getTopRight(mounts).dx,
          tester.getTopRight(connections).dx,
        );
        await tester.tap(mounts);
        await tester.pumpAndSettle();
        expect(find.text(WorkspacePage.connections.label), findsNWidgets(2));
        await tester.pumpWidget(const SizedBox());
      },
      variant: TargetPlatformVariant({TargetPlatform.linux}),
    );
  }
}
