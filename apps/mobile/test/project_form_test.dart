import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/resources/project_form.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';

import 'support/drafts.dart';

void main() {
  for (final kind in ['register_project', 'update_project', 'remove_project']) {
    testWidgets('$kind recovers the original request after reopening', (
      tester,
    ) async {
      final dispatched = Completer<Map<String, dynamic>>();
      final connection = RecoveryConnection();
      final sent = <Map<String, dynamic>>[];
      final host = HostConnection.test(
        id: 'host',
        label: 'Host',
        connection: connection,
        command: (command, data) {
          expect(command, kind);
          sent.add(data!);
          return dispatched.future;
        },
      );
      addTearDown(host.dispose);
      const project = <String, dynamic>{
        'id': 'project',
        'name': 'Original',
        'path': '/project',
        'appearance': <String, dynamic>{},
      };
      final removing = kind == 'remove_project';
      await tester.pumpWidget(
        MaterialApp(
          theme: SailryTheme.of(Brightness.light),
          locale: const Locale('en'),
          supportedLocales: AppLocalizations.supportedLocales,
          localizationsDelegates: AppLocalizations.localizationsDelegates,
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => showAppSheet(
                  context,
                  'Project',
                  child: removing
                      ? ProjectForm.remove(host: host, project: project)
                      : ProjectForm(
                          host: host,
                          project: kind == 'update_project' ? project : null,
                        ),
                ),
                child: const Text('Open'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      if (!removing) {
        await tester.enterText(find.byType(TextField).at(0), 'Edited');
        await tester.enterText(find.byType(TextField).at(1), '/project');
      }
      final button = find.descendant(
        of: find.byType(ProjectForm),
        matching: find.byType(FilledButton),
      );
      await tester.tap(button);
      await tester.pump();
      expect(tester.widget<FilledButton>(button).onPressed, isNull);
      expect(sent, hasLength(1));
      dispatched.completeError(
        const CommandFailure('outcome_unknown', request: 'original'),
      );
      await tester.pumpAndSettle();
      // Let the unknown-result toast close before dismissing the form.
      await tester.pump(const Duration(seconds: 3));
      await tester.pumpAndSettle();
      Navigator.of(tester.element(find.byType(ProjectForm))).pop();
      await tester.pumpAndSettle();
      await tester.tap(find.text('Open'));
      await tester.pumpAndSettle();
      final form = tester.element(find.byType(ProjectForm));
      expect(find.text(form.tr('conversationCheckResult')), findsOneWidget);
      if (!removing) {
        final name = tester.widget<TextField>(find.byType(TextField).at(0));
        expect(name.controller?.text, 'Edited');
        expect(name.enabled, isFalse);
      }
      await tester.tap(button);
      await tester.pumpAndSettle();
      expect(connection.requests, ['original']);
      expect(sent, hasLength(1));
      expect(find.byType(ProjectForm), findsNothing);
      await tester.pumpWidget(const SizedBox.shrink());
    });
  }
}
