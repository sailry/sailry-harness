import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/features/resources/files_page.dart';
import 'package:sailry_mobile/features/resources/git_page.dart';
import 'package:sailry_mobile/features/resources/hosts_page.dart';
import 'package:sailry_mobile/features/resources/resources_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

Future<void> mount(
  WidgetTester tester,
  Widget page, {
  Brightness brightness = Brightness.light,
  double scale = 1,
}) async {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = const Size(390, 844);
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(
          context,
        ).copyWith(textScaler: TextScaler.linear(scale)),
        child: child!,
      ),
      home: page,
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> tapText(WidgetTester tester, String value) async {
  final target = find.text(value).last;
  await tester.ensureVisible(target);
  await tester.tap(target);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('workspace filtering and standalone files', (tester) async {
    await mount(tester, const ResourcesPage());
    expect(find.text(tr('browser')), findsNothing);
    expect(find.text(tr('ports')), findsOneWidget);
    expect(find.text(tr('ssh')), findsNothing);
    expect(find.text(tr('database')), findsNothing);
    await tester.tap(find.byTooltip('${tr('selectHost')}: Studio'));
    await tester.pumpAndSettle();
    await tapText(tester, 'Build Server');
    expect(find.text('sailry-api'), findsOneWidget);
    await tapText(tester, 'sailry-api');
    await tapText(tester, 'sailry');
    await tapText(tester, 'main');
    await tapText(tester, tr('select'));
    expect(find.text('sailry'), findsOneWidget);
    expect(find.text('main'), findsOneWidget);
    await tapText(tester, tr('files'));
    expect(find.byType(FilesPage), findsOneWidget);
    expect(find.byType(FloatingNavigation), findsNothing);
    expect(find.text('README.md'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('host identity and offline metrics', (tester) async {
    await mount(tester, const HostsPage());
    await tapText(tester, 'Studio');
    expect(find.text(tr('manageHost')), findsNothing);
    expect(find.text('Studio'), findsOneWidget);
    expect(find.text(tr('hostProjects')), findsOneWidget);
    await tester.tapAt(const Offset(8, 80));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    await tapText(tester, 'MacBook Air');
    expect(find.text(tr('hostOffline')), findsOneWidget);
    expect(find.text(tr('activity')), findsNothing);
    await tapText(tester, tr('retry'));
    expect(find.text(tr('retryNote')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('preview pairing validates without connecting', (tester) async {
    await mount(tester, const HostsPage());
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(ListTile, tr('pair')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextFormField), '123');
    await tapText(tester, tr('pairDemo'));
    expect(find.text(tr('pairInvalid')), findsOneWidget);
    await tester.enterText(find.byType(TextFormField), '123456');
    await tapText(tester, tr('pairDemo'));
    expect(find.text('Preview Host'), findsOneWidget);
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    expect(find.text('Preview Host'), findsNWidgets(2));
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('preview Git staging, commits, and branches', (tester) async {
    await mount(tester, const GitPage());
    expect(find.text('+17'), findsOneWidget);
    expect(find.text('−3'), findsOneWidget);
    await tapText(tester, tr('stage'));
    await tapText(tester, tr('commit'));
    await tester.enterText(find.byType(TextFormField), 'Refine login spacing');
    await tapText(tester, tr('commitPreview'));
    await tester.drag(
      find.byType(SingleChildScrollView).first,
      const Offset(0, 1000),
    );
    await tester.pumpAndSettle();
    expect(find.text('+0'), findsOneWidget);
    await tapText(tester, tr('gitHistory'));
    expect(find.text('Refine login spacing'), findsOneWidget);
    await tapText(tester, tr('gitBranches'));
    await tapText(tester, tr('gitCreateBranch'));
    await tester.enterText(find.byType(TextFormField), 'feature/mobile');
    await tapText(tester, tr('create'));
    await tapText(tester, 'feature/mobile');
    await tapText(tester, tr('gitSwitch'));
    await tapText(tester, tr('confirm'));
    expect(find.text('feature/mobile'), findsNWidgets(2));
    expect(find.text(tr('gitCurrent')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('search and editing stay within the workspace', (tester) async {
    await mount(tester, const ResourcesPage());
    await tapText(tester, tr('files'));
    expect(find.byTooltip(tr('selectWorkspace')), findsNothing);
    await tester.tap(find.byTooltip(tr('searchFiles')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'readme');
    await tester.pumpAndSettle();
    expect(find.text('src/pages/Login.tsx'), findsNothing);
    await tapText(tester, 'README.md');
    await tapText(tester, tr('edit'));
    await tester.enterText(find.byType(TextField), '# Local preview edit');
    await tester.tapAt(const Offset(8, 80));
    await tester.pumpAndSettle();
    await tapText(tester, 'README.md');
    expect(find.text(tr('unsaved')), findsOneWidget);
    await tapText(tester, tr('edit'));
    expect(find.text('# Local preview edit'), findsOneWidget);
    await tapText(tester, tr('save'));
    await tapText(tester, 'README.md');
    expect(find.text('Local preview edit'), findsOneWidget);
    expect(
      tester.widget<MarkdownContent>(find.byType(MarkdownContent)).data,
      '# Local preview edit',
    );
    await tester.tapAt(const Offset(8, 80));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('back')));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(SelectorCard, 'sailry-web'));
    await tester.pumpAndSettle();
    await tapText(tester, 'main');
    await tapText(tester, tr('select'));
    await tapText(tester, tr('files'));
    await tapText(tester, 'README.md');
    expect(find.text('# Local preview edit'), findsNothing);
    expect(
      tester.widget<MarkdownContent>(find.byType(MarkdownContent)).data,
      tr('markdownExample'),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('narrow layouts in both themes', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 740);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    for (final brightness in Brightness.values) {
      for (final page in [
        const HostsPage(),
        const ResourcesPage(),
        const GitPage(),
        const FilesPage(),
      ]) {
        await tester.pumpWidget(
          MaterialApp(theme: SailryTheme.of(brightness), home: page),
        );
        await tester.pumpAndSettle();
        expect(
          tester.takeException(),
          isNull,
          reason: '${page.runtimeType} in $brightness',
        );
      }
    }
  });
}
