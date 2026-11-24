import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/scope_picker.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

Widget screen(AppSession session, Widget child) => SessionScope(
  session: session,
  child: MaterialApp(
    theme: SailryTheme.of(Brightness.light),
    locale: const Locale('en'),
    supportedLocales: AppLocalizations.supportedLocales,
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    home: Scaffold(body: SingleChildScrollView(child: child)),
  ),
);

Future<void> release(WidgetTester tester, AppSession session) async {
  await tester.pumpWidget(const SizedBox.shrink());
  await session.close();
  session.dispose();
}

ScopeRow row(String title, {bool selected = false, VoidCallback? onTap}) =>
    ScopeRow(
      key: ValueKey(title),
      leading: const AppIcon('folder', size: scopeIconSize),
      title: title,
      selected: selected,
      onTap: onTap,
    );

void main() {
  testWidgets('sections expand the current choice and toggle from headers', (
    tester,
  ) async {
    final first = HostConnection.test(id: 'first', label: 'First');
    final second = HostConnection.test(id: 'second', label: 'Second');
    final session = AppSession.test(hosts: [first, second]);
    final taps = <String>[];
    await tester.pumpWidget(
      screen(
        session,
        ScopePicker(
          rows: [row('All', onTap: () => taps.add('all'))],
          sections: [
            (
              host: first,
              expanded: false,
              children: [row('Alpha', onTap: () => taps.add('alpha'))],
            ),
            (
              host: second,
              expanded: true,
              children: [
                row('Beta', selected: true, onTap: () => taps.add('beta')),
              ],
            ),
          ],
        ),
      ),
    );
    expect(find.text('Alpha'), findsNothing);
    expect(find.text('Beta'), findsOneWidget);
    expect(
      tester.widget<ScopeRow>(find.byKey(const ValueKey('Beta'))).selected,
      isTrue,
    );
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('Beta')),
        matching: find.byWidgetPredicate(
          (widget) => widget is AppIcon && widget.name == 'check',
        ),
      ),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const ValueKey('host-row-first')));
    await tester.pumpAndSettle();
    expect(find.text('Alpha'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey('host-row-second')));
    await tester.pumpAndSettle();
    expect(find.text('Beta'), findsNothing);

    await tester.tap(find.text('All'));
    await tester.tap(find.text('Alpha'));
    expect(taps, ['all', 'alpha']);
    await release(tester, session);
  });

  testWidgets('a lone section starts open and disabled rows ignore taps', (
    tester,
  ) async {
    final host = HostConnection.test(id: 'host', label: 'Host');
    final session = AppSession.test(hosts: [host]);
    await tester.pumpWidget(
      screen(
        session,
        ScopePicker(
          sections: [
            (host: host, expanded: false, children: [row('Offline')]),
          ],
        ),
      ),
    );
    expect(find.text('Offline'), findsOneWidget);
    final offline = tester.widget<ScopeRow>(
      find.byKey(const ValueKey('Offline')),
    );
    expect(offline.enabled, isFalse);
    expect(
      tester
          .widget<InkWell>(
            find.descendant(
              of: find.byKey(const ValueKey('Offline')),
              matching: find.byType(InkWell),
            ),
          )
          .onTap,
      isNull,
    );
    await release(tester, session);
  });

  testWidgets('the host picker marks the current host and returns the choice', (
    tester,
  ) async {
    final first = HostConnection.test(id: 'first', label: 'First');
    final second = HostConnection.test(id: 'second', label: 'Second');
    final session = AppSession.test(hosts: [first, second]);
    String? chosen;
    await tester.pumpWidget(
      screen(
        session,
        Builder(
          builder: (context) => TextButton(
            onPressed: () async {
              chosen = await showHostPicker(
                context,
                session,
                selected: first.id,
              );
            },
            child: const Text('Pick'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Pick'));
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<ScopeRow>(
            find.ancestor(
              of: find.byKey(const ValueKey('host-row-first')),
              matching: find.byType(ScopeRow),
            ),
          )
          .selected,
      isTrue,
    );
    await tester.tap(find.byKey(const ValueKey('host-row-second')));
    await tester.pumpAndSettle();
    expect(chosen, second.id);
    await release(tester, session);
  });
}
