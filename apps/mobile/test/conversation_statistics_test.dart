import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/statistics.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  testWidgets('shows end-to-end output rate and its timing coverage', (
    tester,
  ) async {
    Future<void> show(Map<String, dynamic> data) => tester.pumpWidget(
      MaterialApp(
        locale: const Locale('en'),
        supportedLocales: AppLocalizations.supportedLocales,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        home: Scaffold(body: ConversationStatistics(data: data)),
      ),
    );

    const legacy = {
      'responses': 5,
      'generation': {
        'elapsed_us': 1000000,
        'output_tokens': 40,
        'responses': 4,
      },
    };
    await show({
      ...legacy,
      'throughput': {
        'elapsed_us': 2000000,
        'output_tokens': 40,
        'responses': 2,
      },
    });
    expect(find.text('End-to-end output rate'), findsOneWidget);
    expect(find.text('20.0 tok/s'), findsOneWidget);
    expect(find.text('2 / 5'), findsOneWidget);

    await show(legacy);
    expect(find.text('Model responses'), findsOneWidget);
    expect(find.byKey(const ValueKey('conversationStatsSpeed')), findsNothing);
    expect(
      find.byKey(const ValueKey('conversationStatsTimingCoverage')),
      findsNothing,
    );
  });
}
