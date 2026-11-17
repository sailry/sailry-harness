import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api.dart';
import 'package:sailry_mobile/features/resources/host/host_sheet.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';

class RemovalController extends Fake implements Controller {
  final pending = Completer<void>();
  int removals = 0;

  @override
  Future<void> forget({required String address}) {
    removals++;
    return pending.future;
  }

  @override
  Future<void> close() async {}

  @override
  void dispose() {}
}

void main() {
  for (final reopen in [false, true]) {
    testWidgets('pending removal blocks host switching (reopen: $reopen)', (
      tester,
    ) async {
      final controller = RemovalController();
      final selected = HostConnection.test(id: 'selected', label: 'Current');
      final target = HostConnection.test(
        id: 'target',
        label: 'Offline',
        connected: false,
      );
      final session = AppSession.test(
        hosts: [selected, target],
        controller: controller,
      );
      await tester.pumpWidget(
        SessionScope(
          session: session,
          child: MaterialApp(
            locale: const Locale('en'),
            supportedLocales: AppLocalizations.supportedLocales,
            localizationsDelegates: AppLocalizations.localizationsDelegates,
            home: Scaffold(
              body: Builder(
                builder: (context) => TextButton(
                  onPressed: () => showHostSheet(context, onPair: () {}),
                  child: const Text('Hosts'),
                ),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Hosts'));
      await tester.pumpAndSettle();
      final remove = find.byKey(const ValueKey('remove-host-target'));
      await tester.tap(remove);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 350));
      await tester.tap(find.widgetWithText(FilledButton, 'Remove host'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 350));
      expect(controller.removals, 1);
      expect(find.byType(CircularProgressIndicator), findsOneWidget);

      if (reopen) {
        await tester.tapAt(const Offset(4, 4));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 350));
        await tester.tap(find.text('Hosts'));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 350));
        expect(find.byType(CircularProgressIndicator), findsOneWidget);
      }

      // A disabled nested icon must not hand its tap to the host row behind it.
      await tester.tapAt(tester.getCenter(remove));
      await tester.tapAt(
        tester.getCenter(find.byKey(const ValueKey('host-row-target'))),
      );
      await tester.pump();
      session.selectHost(target.id);
      expect(controller.removals, 1);
      expect(session.selectedHost, same(selected));
      expect(find.text('Offline'), findsOneWidget);

      controller.pending.complete();
      await tester.pumpAndSettle();
      expect(session.host('target'), isNull);
      expect(session.selectedHost, same(selected));
      expect(find.text('Offline'), findsNothing);
      if (!reopen) expect(find.text('Host removed'), findsOneWidget);
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      await tester.pumpWidget(const SizedBox.shrink());
      await session.close();
      session.dispose();
    });
  }
}
