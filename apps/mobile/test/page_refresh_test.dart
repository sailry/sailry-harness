import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/ui/kit.dart';

void main() {
  Future<int> pull(WidgetTester tester, {Widget? failure}) async {
    var refreshes = 0;
    await tester.pumpWidget(
      MaterialApp(
        home: PageFrame(
          title: 'Tasks',
          failure: failure,
          onRefresh: () async => refreshes++,
          child: const Text('Rows'),
        ),
      ),
    );
    // Pull from empty space below the content, as on a short list.
    await tester.flingFrom(const Offset(200, 400), const Offset(0, 300), 1000);
    await tester.pumpAndSettle();
    return refreshes;
  }

  testWidgets('pulling down refreshes a short list', (tester) async {
    expect(await pull(tester), 1);
  });

  testWidgets('pulling down retries an offline page', (tester) async {
    expect(
      await pull(tester, failure: const FailureState(message: 'Offline')),
      1,
    );
    expect(find.text('Offline'), findsOneWidget);
  });
}
