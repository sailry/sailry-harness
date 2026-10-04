import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/statistics.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'live_conversations_test.dart' as fixture;

Future<void> mount(
  WidgetTester tester,
  Map<String, dynamic> data, {
  double scale = 1,
}) async {
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(Brightness.dark),
      home: MediaQuery(
        data: MediaQueryData(textScaler: TextScaler.linear(scale)),
        child: Scaffold(
          body: SingleChildScrollView(
            child: ConversationStatistics(data: data),
          ),
        ),
      ),
    ),
  );
}

void main() {
  testWidgets(
    'statistics open from the header and update outside the transcript',
    (tester) async {
      final connection = fixture.ConnectionFixture();
      final record = fixture.session();
      final host = HostConnection.test(
        id: 'node',
        label: 'Node',
        connection: connection,
        snapshot: {
          'sessions': [record],
        },
        command: (_, _) async => {},
      );
      final app = AppSession.test(hosts: [host]);
      await fixture.mount(
        tester,
        LiveConversationPage(
          host: host,
          sessionId: 'session',
          initialSession: record,
        ),
        app,
      );
      Map<String, dynamic> update(int input) {
        final view = fixture.view(
          entries: [fixture.entry('answer', 'assistant', 'The answer')],
        );
        view['snapshot']['statistics'] = {
          'usage': {'input': input, 'output': 10},
        };
        return view;
      }

      connection.updates.emit(update(100));
      await tester.pumpAndSettle();
      expect(find.byType(ConversationStatistics), findsNothing);
      expect(find.text('The answer'), findsOneWidget);
      final info = find.byKey(const ValueKey('conversation-info'));
      expect(tester.getCenter(info).dx, greaterThan(215));
      expect(tester.getCenter(info).dy, lessThan(100));
      await tester.tap(info);
      await tester.pumpAndSettle();
      expect(find.byType(ConversationStatistics), findsOneWidget);
      expect(find.byType(ExpansionTile), findsNothing);
      expect(find.text('110'), findsOneWidget);
      connection.updates.emit(update(200));
      await tester.pumpAndSettle();
      expect(find.text('210'), findsOneWidget);
      expect(find.text('110'), findsNothing);
      Navigator.of(tester.element(find.byType(ConversationStatistics))).pop();
      await tester.pumpAndSettle();
      expect(find.byType(ConversationStatistics), findsNothing);
      expect(find.text('The answer'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
      await app.close();
      app.dispose();
    },
  );

  testWidgets('unknown metrics remain absent', (tester) async {
    await mount(tester, const {});
    expect(find.text(tr('conversationStatsEmpty')), findsOneWidget);
    await mount(tester, const {
      'turns': 2,
      'responses': 1,
      'usage': null,
      'cost': null,
      'generation': null,
    });
    expect(find.text('2'), findsOneWidget);
    expect(find.textContaining('\$'), findsNothing);
    expect(find.textContaining('tok/s'), findsNothing);
    expect(find.text(tr('conversationStatsResponses')), findsOneWidget);
    expect(find.text(tr('conversationStatsContext')), findsNothing);
    expect(find.text(tr('conversationStatsInput')), findsNothing);
  });

  testWidgets('totals, subsets, and partial coverage at narrow widths', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 900);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await mount(tester, const {
      'turns': 3,
      'responses': 2,
      'usage': {
        'input': 1200,
        'output': 300,
        'cached_input': 800,
        'reasoning': 100,
      },
      'context_tokens': 1500,
      'cost': {
        'usd_micros': 1234567,
        'responses': 1,
        'breakdown': {
          'input': 1000000,
          'output': 230000,
          'cache_read': 4567,
          'cache_write': 0,
        },
      },
      'generation': {
        'output_tokens': 100,
        'elapsed_us': 4000000,
        'responses': 1,
      },
    }, scale: 1.6);
    expect(find.text('1.5k'), findsNWidgets(2));
    expect(find.text('25.0 tok/s'), findsOneWidget);
    expect(find.text('\$1.23+'), findsOneWidget);
    expect(find.text('67%'), findsOneWidget);
    expect(find.text('2.4k'), findsNothing);
    for (final (key, value) in [
      ('conversationStatsInput', '1.2k'),
      ('conversationStatsOutput', '300'),
      ('conversationStatsCached', '800'),
      ('conversationStatsReasoning', '100'),
      ('conversationStatsContext', '1.5k'),
      ('conversationStatsCostCoverage', '1 / 2'),
      ('conversationStatsTimingCoverage', '1 / 2'),
    ]) {
      expect(
        find.descendant(
          of: find.byKey(ValueKey(key)),
          matching: find.text(value),
        ),
        findsOneWidget,
      );
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets('zero differs from missing measurements', (tester) async {
    await mount(tester, const {
      'turns': 1,
      'responses': 1,
      'usage': {'input': 0, 'output': 0, 'cached_input': 0, 'reasoning': 0},
      'context_tokens': 0,
      'cost': {'usd_micros': 0, 'responses': 1},
      'generation': {'elapsed_us': 0, 'output_tokens': 0, 'responses': 0},
    });
    expect(find.text('\$0.00'), findsOneWidget);
    expect(find.textContaining('tok/s'), findsNothing);
    expect(find.textContaining('%'), findsNothing);
    expect(
      find.descendant(
        of: find.byKey(const ValueKey('conversationStatsContext')),
        matching: find.text('0'),
      ),
      findsOneWidget,
    );
    expect(find.textContaining('NaN'), findsNothing);
  });
}
