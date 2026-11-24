import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/disclosure.dart';
import 'package:sailry_mobile/l10n/strings.dart';

List<Widget> _steps(int count) => [
  for (var index = 0; index < count; index++)
    SizedBox(key: ValueKey(index), height: 32, child: Text('step $index')),
];

String _earlier(int count) =>
    tr('conversationEarlierSteps').replaceAll('{count}', '$count');

Future<void> _show(WidgetTester tester, Widget child) => tester.pumpWidget(
  MaterialApp(home: Scaffold(body: SingleChildScrollView(child: child))),
);

void main() {
  testWidgets('expanded steps show the latest rows without an inner scroll', (
    tester,
  ) async {
    await _show(tester, StepList(children: _steps(70)));
    expect(
      find.descendant(
        of: find.byType(StepList),
        matching: find.byType(Scrollable),
      ),
      findsNothing,
    );
    expect(find.text('step 69'), findsOneWidget);
    expect(find.text('step 62'), findsOneWidget);
    expect(find.text('step 61'), findsNothing);

    await tester.tap(find.text(_earlier(50)));
    await tester.pump();
    expect(find.text('step 12'), findsOneWidget);
    expect(find.text('step 11'), findsNothing);

    await tester.tap(find.text(_earlier(12)));
    await tester.pump();
    expect(find.text('step 0'), findsOneWidget);
    expect(find.textContaining(_earlier(1).split('1').first), findsNothing);
  });

  testWidgets('short step lists show every row without a control', (
    tester,
  ) async {
    await _show(tester, StepList(children: _steps(8)));
    expect(find.text('step 0'), findsOneWidget);
    expect(find.byKey(const ValueKey('earlier-steps')), findsNothing);
  });

  testWidgets('new steps keep the latest rows in view', (tester) async {
    await _show(tester, StepList(children: _steps(10)));
    expect(find.text('step 9'), findsOneWidget);
    await _show(tester, StepList(children: _steps(11)));
    expect(find.text('step 10'), findsOneWidget);
    expect(find.text('step 2'), findsNothing);
    expect(find.text(_earlier(3)), findsOneWidget);
  });

  testWidgets('collapsed steps mount on demand', (tester) async {
    await _show(
      tester,
      StepDisclosure(
        key: const ValueKey('steps'),
        label: '40 steps',
        children: _steps(40),
      ),
    );
    expect(find.byType(StepList, skipOffstage: false), findsNothing);
    await tester.tap(find.text('40 steps'));
    await tester.pumpAndSettle();
    expect(find.text('step 39'), findsOneWidget);
  });
}
