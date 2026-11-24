import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/change_preview.dart';
import 'package:sailry_mobile/l10n/strings.dart';

void main() {
  testWidgets('a long diff previews its first lines and opens in place', (
    tester,
  ) async {
    final diff = [
      '@@ -1,20 +1,20 @@',
      for (var line = 0; line < 20; line++) '+line $line',
    ].join('\n');
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: ChangePreview(
              changes: {
                'files': [
                  {'path': 'a.txt', 'kind': 'modified', 'diff': diff},
                ],
              },
            ),
          ),
        ),
      ),
    );
    expect(find.textContaining('line 10'), findsOneWidget);
    expect(find.textContaining('line 11'), findsNothing);
    final vertical = find.byWidgetPredicate(
      (widget) =>
          widget is SingleChildScrollView &&
          widget.scrollDirection == Axis.vertical,
    );
    expect(vertical, findsOneWidget);

    await tester.tap(
      find.text(tr('contentShowAll').replaceAll('{count}', '21')),
    );
    await tester.pump();
    expect(find.textContaining('line 19'), findsOneWidget);
    await tester.tap(find.text(tr('contentShowLess')));
    await tester.pump();
    expect(find.textContaining('line 11'), findsNothing);
  });
}
