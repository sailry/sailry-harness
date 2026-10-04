import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/settings/settings_page.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  setUp(() {
    final binding = TestWidgetsFlutterBinding.ensureInitialized();
    binding.platformDispatcher.accessibilityFeaturesTestValue =
        const FakeAccessibilityFeatures(disableAnimations: true);
  });
  tearDown(() {
    TestWidgetsFlutterBinding.ensureInitialized().platformDispatcher
        .clearAccessibilityFeaturesTestValue();
  });

  testWidgets('primary destinations at narrow widths', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 740);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(const SailryApp(preview: true));
    await tester.pumpAndSettle();
    expect(
      find.descendant(
        of: find.byType(FloatingNavigation),
        matching: find.byType(TextButton),
      ),
      findsNWidgets(4),
    );
    for (var index = 0; index < 4; index++) {
      await tester.tap(find.byKey(ValueKey('tab-$index')));
      await tester.pumpAndSettle();
      expect(find.byType(FloatingNavigation), findsOneWidget);
      expect(tester.takeException(), isNull);
    }
    expect(find.byType(SettingsPage), findsOneWidget);
  });

  testWidgets('floating navigation keeps the last task visible', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(const SailryApp(preview: true));
    await tester.pumpAndSettle();
    expect(find.text(tr('preview')), findsNothing);
    final viewport = find.byType(SingleChildScrollView).first;
    final navigation = find.byType(FloatingNavigation);
    expect(tester.getBottomRight(viewport).dy, 844);
    expect(tester.getTopLeft(navigation).dy, lessThan(844));
    await tester.drag(viewport, const Offset(0, -900));
    await tester.pumpAndSettle();
    final lastTask = find.text(tr('taskDone'));
    expect(lastTask.hitTestable(), findsOneWidget);
    expect(
      tester.getBottomRight(lastTask).dy,
      lessThan(tester.getTopLeft(navigation).dy),
    );
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('host details reuse settings and configuration', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(390, 844);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final settings = <String, Map<String, dynamic>>{
      for (final id in ['studio', 'build'])
        id: {
          'revision': 0,
          'enabled': false,
          'auto_write': false,
          'context_bytes': 8192,
          'review_after_days': 30,
        },
    };
    final writes = <String>[];
    HostConnection host(String id, String label) => HostConnection.test(
      id: id,
      label: label,
      command: (kind, data) async {
        switch (kind) {
          case 'read_host_metrics':
            return {'kind': 'host_metrics', 'data': <String, dynamic>{}};
          case 'read_memory_settings':
            return {'kind': 'memory_settings', 'data': settings[id]};
          case 'save_memory_settings':
            final next = Map<String, dynamic>.from(data!['settings']);
            expect(next['revision'], settings[id]!['revision']);
            writes.add(id);
            next['revision'] = (next['revision'] as int) + 1;
            settings[id] = next;
            return {'kind': 'memory_settings', 'data': next};
          default:
            throw StateError('Unexpected command: $kind');
        }
      },
    );
    final session = AppSession.test(
      hosts: [host('studio', 'Studio'), host('build', 'Build Server')],
    );
    await tester.pumpWidget(SailryApp(session: session));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-3')));
    await tester.pumpAndSettle();
    final owner = tester.state(find.byType(SettingsPage));
    final notifications = find.widgetWithText(
      SwitchListTile,
      tr('completionAlerts'),
    );
    await tester.tap(notifications);
    await tester.tap(find.text(tr('memorySettings')));
    await tester.pumpAndSettle();
    final enabled = find.widgetWithText(SwitchListTile, tr('settingsEnabled'));
    expect(tester.widget<SwitchListTile>(enabled).value, isFalse);
    await tester.tap(enabled);
    await tester.enterText(
      find.widgetWithText(TextField, tr('settingsMemoryReview')),
      '45',
    );
    await tester.tap(find.widgetWithText(FilledButton, tr('save')));
    await tester.pumpAndSettle();
    expect(writes, ['studio']);
    expect(settings['studio']!['enabled'], isTrue);
    expect(settings['build']!['enabled'], isFalse);
    await tester.tap(find.byTooltip('${tr('selectHost')}: Studio'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Build Server'));
    await tester.pumpAndSettle();
    expect(session.selectedHost!.id, 'build');
    await tester.tap(find.text(tr('memorySettings')));
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(enabled).value, isFalse);
    expect(
      tester
          .widget<TextField>(
            find.widgetWithText(TextField, tr('settingsMemoryReview')),
          )
          .controller!
          .text,
      '30',
    );
    await tester.tapAt(const Offset(8, 80));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('tab-1')));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip(tr('selectHost')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Studio'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Studio'));
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<FloatingNavigation>(find.byType(FloatingNavigation))
          .selected,
      3,
    );
    expect(tester.state(find.byType(SettingsPage)), same(owner));
    expect(find.byTooltip('${tr('selectHost')}: Studio'), findsOneWidget);
    expect(tester.widget<SwitchListTile>(notifications).value, isFalse);
    await tester.tap(find.text(tr('memorySettings')));
    await tester.pumpAndSettle();
    expect(tester.widget<SwitchListTile>(enabled).value, isTrue);
    expect(
      tester
          .widget<TextField>(
            find.widgetWithText(TextField, tr('settingsMemoryReview')),
          )
          .controller!
          .text,
      '45',
    );
    expect(writes, ['studio']);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
    expect(tester.takeException(), isNull);
  });

  testWidgets('standalone terminal with scrolling keys', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(320, 740);
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: const TerminalPage(),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.byType(FloatingNavigation), findsNothing);
    for (final key in ['Esc', 'Tab', 'Ctrl', 'Alt', 'Shift', 'Cmd']) {
      expect(find.text(key), findsOneWidget);
    }
    expect(tester.getSize(find.widgetWithText(TextButton, 'Cmd')).height, 40);
    expect(
      tester.getCenter(find.text('Esc')).dy,
      tester.getCenter(find.text('Cmd')).dy,
    );
    await tester.enterText(
      find.byKey(const ValueKey('terminal-draft')),
      'echo preview',
    );
    await tester.testTextInput.receiveAction(TextInputAction.done);
    await tester.pump();
    expect(find.text(tr('terminalUnavailable')), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });
}
