import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/disclosure.dart';

const _rowHeight = 32.0;

List<Widget> _steps(int count) => [
  for (var index = 0; index < count; index++)
    SizedBox(
      key: ValueKey(index),
      height: _rowHeight,
      child: Text('step $index'),
    ),
];

Future<void> _show(WidgetTester tester, int count) => tester.pumpWidget(
  MaterialApp(
    home: Scaffold(
      body: Align(
        alignment: Alignment.topCenter,
        child: StepList(children: _steps(count)),
      ),
    ),
  ),
);

void main() {
  // The default test surface is 800x600, so 40% of the screen is 240.
  const bound = 240.0;

  testWidgets('a short process keeps its natural height', (tester) async {
    await _show(tester, 3);
    await tester.pumpAndSettle();
    expect(tester.getSize(find.byType(StepList)).height, 3 * _rowHeight);
    expect(find.text('step 0'), findsOneWidget);
    expect(find.text('step 2'), findsOneWidget);
  });

  testWidgets('a long process is bounded, opens on its latest step and '
      'builds only near the viewport', (tester) async {
    await _show(tester, 200);
    await tester.pumpAndSettle();
    expect(tester.getSize(find.byType(StepList)).height, bound);
    final last = find.byKey(const ValueKey(199)).hitTestable();
    expect(last, findsOneWidget);
    expect(
      tester.getBottomLeft(last).dy,
      closeTo(tester.getBottomLeft(find.byType(StepList)).dy, 1),
    );
    expect(find.text('step 0'), findsNothing);
    expect(find.text('step 100'), findsNothing);
  });

  testWidgets('the list follows new steps until the reader scrolls back', (
    tester,
  ) async {
    await _show(tester, 40);
    await tester.pumpAndSettle();

    await _show(tester, 41);
    await tester.pumpAndSettle();
    expect(find.text('step 40').hitTestable(), findsOneWidget);

    // Reading earlier steps: new ones arrive without moving the view.
    await tester.drag(find.byType(ListView), const Offset(0, 5 * _rowHeight));
    await tester.pumpAndSettle();
    expect(find.text('step 40').hitTestable(), findsNothing);
    final earlier = find.text('step 34').hitTestable();
    final reading = tester.getTopLeft(earlier).dy;
    await _show(tester, 45);
    await tester.pumpAndSettle();
    expect(find.text('step 44').hitTestable(), findsNothing);
    expect(tester.getTopLeft(earlier).dy, reading);

    // Returning to the end resumes following.
    await tester.drag(find.byType(ListView), const Offset(0, -2000));
    await tester.pumpAndSettle();
    expect(find.text('step 44').hitTestable(), findsOneWidget);
    await _show(tester, 46);
    await tester.pumpAndSettle();
    expect(find.text('step 45').hitTestable(), findsOneWidget);
  });

  testWidgets('the bound tracks the screen height and keeps the latest step', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(390, 2000);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.reset);
    await _show(tester, 200);
    await tester.pumpAndSettle();
    expect(tester.getSize(find.byType(StepList)).height, 420);

    tester.view.physicalSize = const Size(390, 300);
    await tester.pumpAndSettle();
    expect(tester.getSize(find.byType(StepList)).height, 160);
    expect(find.byKey(const ValueKey(199)).hitTestable(), findsOneWidget);
  });
}
