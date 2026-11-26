import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:twodrive_dav/client.dart';
import 'package:twodrive_dav/overview_page.dart';

void main() {
  for (final (width, scale, columns) in [
    (1200.0, 1.0, 4),
    (1600.0, 1.5, 4),
    (2050.0, 2.0, 4),
    (840.0, 1.0, 2),
    (1200.0, 1.5, 2),
    (720.0, 2.0, 1),
  ]) {
    testWidgets('overview aligns cloud cards at $width / $scale', (
      tester,
    ) async {
      tester.view.physicalSize = Size(width, 2400);
      tester.view.devicePixelRatio = 1;
      tester.platformDispatcher.textScaleFactorTestValue = scale;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
      final providers = ['icloud', 'onedrive', 'baidu', 'google'];
      final client = StorageClient()
        ..available = true
        ..profiles = [
          for (final kind in providers)
            {'id': kind, 'kind': kind, 'name': kind},
        ]
        ..status = {
          'mounts': [
            for (final kind in providers) {'profile': kind},
          ],
        };
      client.quotas.addAll({
        'icloud': {
          'usage': {'used': 400, 'total': 1000},
          'stale': true,
        },
        'onedrive': {
          'usage': {'used': 200, 'total': 1000},
        },
        'google': {
          'usage': {'free': 600},
        },
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: OverviewPage(
              client: client,
              onNavigate: (_) {},
              onOpenMount: (_) async {},
            ),
          ),
        ),
      );
      final actions = [
        for (final kind in providers)
          find.byKey(ValueKey('overview-account-$kind')),
      ];
      final cards = [
        for (final action in actions)
          tester.getRect(
            find
                .ancestor(
                  of: action,
                  matching: find.byWidgetPredicate(
                    (widget) =>
                        widget is Container && widget.decoration != null,
                  ),
                )
                .first,
          ),
      ];
      for (final (index, card) in cards.indexed) {
        final rowStart = index - index % columns;
        final firstInRow = cards[rowStart];
        expect(card.width, closeTo(cards.first.width, .01));
        expect(card.top, closeTo(firstInRow.top, .01));
        expect(card.bottom, closeTo(firstInRow.bottom, .01));
        expect(
          tester.getRect(actions[index]).bottom,
          closeTo(tester.getRect(actions[rowStart]).bottom, .01),
        );
        if (index > 0 && index % columns == 0) {
          expect(card.top, greaterThan(cards[index - 1].bottom));
        }
      }
      expect(find.text('打开目录'), findsNWidgets(providers.length));
      expect(find.text('容量刷新未完成，显示上次结果'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      client.dispose();
    });
  }

  for (final scale in [1.0, 1.5, 2.0]) {
    for (final busySide in ['backup', 'transfers']) {
      testWidgets('overview previews share both edges: $busySide at $scale', (
        tester,
      ) async {
        tester.view.physicalSize = Size(1200 * scale, 2400);
        tester.view.devicePixelRatio = 1;
        tester.platformDispatcher.textScaleFactorTestValue = scale;
        addTearDown(tester.view.resetPhysicalSize);
        addTearDown(tester.view.resetDevicePixelRatio);
        addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
        final client = StorageClient()..available = true;
        client.status = {
          if (busySide == 'backup')
            'accounts': [
              for (var i = 0; i < 3; i++)
                {
                  'name': 'Example backup $i',
                  'backup': {'folders': 1, 'state': 'scanning'},
                },
            ],
        };
        if (busySide == 'transfers') {
          for (var i = 0; i < 3; i++) {
            client.transfers.add(
              Transfer(i + 1, 'upload', {'path': '/example-$i.txt'}),
            );
          }
        }
        await tester.pumpWidget(
          MaterialApp(
            home: Scaffold(
              body: OverviewPage(
                client: client,
                onNavigate: (_) {},
                onOpenMount: (_) async {},
              ),
            ),
          ),
        );
        Rect bounds(String title) => tester.getRect(
          find
              .ancestor(of: find.text(title), matching: find.byType(Container))
              .first,
        );
        final transfers = bounds('传输概览');
        final backup = bounds('备份概览');
        expect(transfers.top, closeTo(backup.top, .01));
        expect(transfers.bottom, closeTo(backup.bottom, .01));
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox());
        client.dispose();
      });
    }
  }
}
