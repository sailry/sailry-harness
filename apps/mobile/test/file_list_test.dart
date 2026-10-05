import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/file_size.dart';
import 'package:sailry_mobile/features/resources/file_location.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'live_resources_test.dart' as fixture;

void main() {
  test('file sizes handle missing values and unit boundaries', () {
    for (final (bytes, expected) in <(int?, String)>[
      (null, '—'),
      (-1, '—'),
      (0, '0 B'),
      (999, '999 B'),
      (1000, '1 KB'),
      (1536, '1.5 KB'),
      (999950, '1 MB'),
      (2500000, '2.5 MB'),
      (1000000000, '1 GB'),
      (1000000000000, '1 TB'),
    ]) {
      expect(fileSize(bytes), expected, reason: '$bytes bytes');
    }
  });

  testWidgets('formats sizes and navigates through breadcrumbs', (
    tester,
  ) async {
    final paths = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        expect(kind, 'list_directory');
        expect(data!['worktree'], 'tree');
        paths.add(data['path'] as String);
        return {
          'data': {
            'entries': [
              if (data['path'] == '') ...[
                {'name': 'notes.txt', 'kind': 'file', 'size': 1536},
                {'name': 'archive.zip', 'kind': 'file', 'size': 2500000},
                {'name': 'empty.txt', 'kind': 'file', 'size': 0},
                {'name': 'docs', 'kind': 'directory'},
              ] else if (data['path'] == 'docs')
                {'name': 'Draft folder', 'kind': 'directory'},
            ],
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const FilesPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    expect(find.text('1.5 KB'), findsOneWidget);
    expect(find.text('2.5 MB'), findsOneWidget);
    expect(find.text('0 B'), findsOneWidget);
    expect(find.text('null B'), findsNothing);
    await tester.tap(find.text('docs'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Draft folder'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'docs'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, tr('resourceRoot')));
    await tester.pumpAndSettle();
    expect(paths, ['', 'docs', 'docs/Draft folder', 'docs', '']);
    expect(find.text('1.5 KB'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  testWidgets('a failed directory retains parent navigation', (tester) async {
    final paths = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Node',
      snapshot: {
        'worktrees': [fixture.workspace],
      },
      command: (kind, data) async {
        final path = data!['path'] as String;
        paths.add(path);
        if (path == 'blocked') throw const CommandFailure('permission_denied');
        return {
          'data': {
            'entries': [
              {'name': 'blocked', 'kind': 'directory'},
            ],
          },
        };
      },
    );
    final app = AppSession.test(hosts: [host]);
    await fixture.mount(
      tester,
      const FilesPage(hostId: 'node', worktreeId: 'tree'),
      app,
    );
    await tester.tap(find.text('blocked'));
    await tester.pumpAndSettle();
    expect(find.byType(FileLocation), findsOneWidget);
    await tester.tap(find.widgetWithText(TextButton, tr('resourceRoot')));
    await tester.pumpAndSettle();
    expect(paths, ['', 'blocked', '']);
    expect(find.text('blocked'), findsOneWidget);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    app.dispose();
  });

  for (final brightness in Brightness.values) {
    testWidgets('long breadcrumbs fit narrow screens in $brightness', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(320, 740);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      var destination = '';
      const directory = 'docs/Very long parent folder/包含空格的目录 current folder';
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(brightness),
          home: Scaffold(
            body: Center(
              child: Padding(
                padding: const EdgeInsets.all(20),
                child: FileLocation(
                  directory: directory,
                  onNavigate: (path) => destination = path,
                ),
              ),
            ),
          ),
        ),
      );
      final current = find.widgetWithText(TextButton, '包含空格的目录 current folder');
      expect(
        tester
            .getRect(find.byType(FileLocation))
            .contains(tester.getCenter(current)),
        isTrue,
      );
      expect(find.byTooltip(tr('hostParentDirectory')), findsNothing);
      final scroller = find.byType(SingleChildScrollView);
      final position = tester
          .state<ScrollableState>(find.byType(Scrollable))
          .position;
      await tester.drag(scroller, const Offset(240, 0));
      await tester.pumpAndSettle();
      final older = position.pixels;
      expect(older, greaterThan(0));
      await tester.drag(scroller, const Offset(-160, 0));
      await tester.pumpAndSettle();
      expect(position.pixels, lessThan(older));
      final parent = find.widgetWithText(TextButton, 'Very long parent folder');
      await tester.ensureVisible(parent);
      await tester.pumpAndSettle();
      await tester.tap(parent);
      expect(destination, 'docs/Very long parent folder');
      final root = find.widgetWithText(TextButton, tr('resourceRoot'));
      await tester.ensureVisible(root);
      await tester.pumpAndSettle();
      await tester.tap(root);
      expect(destination, '');
      expect(tester.takeException(), isNull);
    });
  }
}
