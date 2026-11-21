import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/ui/theme.dart';

import '../preview/features/conversations/conversation_page.dart';
import '../preview/features/conversations/queue_sheet.dart';
import '../preview/l10n/strings.dart';

void main() {
  testWidgets('preview queue stays reachable during the initial approval', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);

    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        locale: const Locale('zh'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: const ConversationPage(),
      ),
    );
    await tester.pumpAndSettle();

    final priority = find.byKey(const ValueKey('composer-priority'));
    final allow = find.descendant(
      of: priority,
      matching: find.widgetWithText(FilledButton, tr('allowShort')),
    ).hitTestable();
    final deny = find.descendant(
      of: priority,
      matching: find.widgetWithText(TextButton, tr('deny')),
    ).hitTestable();
    void expectApprovalActions() {
      expect(allow, findsOneWidget);
      expect(deny, findsOneWidget);
      expect(tester.widget<FilledButton>(allow).onPressed, isNotNull);
      expect(tester.widget<TextButton>(deny).onPressed, isNotNull);
    }

    final chip = find.byType(QueueChip).hitTestable();
    expectApprovalActions();
    expect(chip, findsOneWidget);
    expect(tester.widget<QueueChip>(chip).state.count, 1);
    expect(tester.widget<QueueChip>(chip).state.paused, isFalse);

    await tester.tap(chip);
    await tester.pumpAndSettle();
    final sheet = find.byType(QueueSheet);
    expect(
      find.descendant(of: sheet, matching: find.text(tr('queueSample'))),
      findsOneWidget,
    );
    await tester.tap(find.widgetWithText(OutlinedButton, tr('pauseQueue')));
    await tester.pumpAndSettle();
    expect(find.widgetWithText(OutlinedButton, tr('resumeQueue')), findsOneWidget);

    Navigator.of(tester.element(sheet)).pop();
    await tester.pumpAndSettle();
    expectApprovalActions();
    expect(chip, findsOneWidget);
    expect(tester.widget<QueueChip>(chip).state.count, 1);
    expect(tester.widget<QueueChip>(chip).state.paused, isTrue);
  });
}
