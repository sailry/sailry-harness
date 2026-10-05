import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  for (final brightness in Brightness.values) {
    for (final connected in [true, false]) {
      testWidgets(
        'home icon foreground follows connection in $brightness/$connected',
        (tester) async {
          final host = HostConnection.test(
            id: 'node',
            label: 'Node',
            connected: connected,
            command: (kind, data) async =>
                throw StateError('Unexpected command: $kind'),
          );
          final session = AppSession.test(hosts: [host]);
          addTearDown(session.dispose);
          final theme = SailryTheme.of(brightness);
          await tester.pumpWidget(
            SessionScope(
              session: session,
              child: MaterialApp(theme: theme, home: const TasksPage()),
            ),
          );
          await tester.pumpAndSettle();
          for (final name in ['server', 'search', 'terminal', 'plus']) {
            final button = find.byWidgetPredicate(
              (widget) => widget is RoundButton && widget.icon == name,
            );
            expect(button, findsOneWidget);
            expect(tester.widget<RoundButton>(button).primary, isFalse);
            final glyph = find.descendant(
              of: button,
              matching: find.byType(AppIcon),
            );
            final expected = name == 'server' || connected
                ? theme.colorScheme.onSurfaceVariant
                : theme.disabledColor;
            expect(IconTheme.of(tester.element(glyph)).color, expected);
            expect(expected.a, greaterThan(0));
            expect(
              tester.widget<RoundButton>(button).onPressed != null,
              name == 'server' || connected,
            );
          }
          expect(tester.takeException(), isNull);
        },
      );
    }

    testWidgets('primary controls keep their contrast in $brightness', (
      tester,
    ) async {
      final theme = SailryTheme.of(brightness);
      await tester.pumpWidget(
        MaterialApp(
          theme: theme,
          home: Scaffold(
            body: Row(
              children: [
                RoundButton(
                  key: const ValueKey('active'),
                  icon: 'send',
                  primary: true,
                  onPressed: () {},
                ),
                const RoundButton(
                  key: ValueKey('disabled'),
                  icon: 'send',
                  primary: true,
                  onPressed: null,
                ),
              ],
            ),
          ),
        ),
      );
      for (final (key, foreground) in [
        ('active', theme.colorScheme.onPrimary),
        ('disabled', theme.disabledColor),
      ]) {
        final glyph = find.descendant(
          of: find.byKey(ValueKey(key)),
          matching: find.byType(AppIcon),
        );
        expect(IconTheme.of(tester.element(glyph)).color, foreground);
      }
      expect(tester.takeException(), isNull);
    });
  }
}
