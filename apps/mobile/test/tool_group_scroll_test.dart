import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/tool_sequence.dart';
import 'package:sailry_mobile/l10n/strings.dart';

import 'tool_sequence_test.dart' as fixture;

void main() {
  for (final count in [2, 30]) {
    testWidgets('group of $count tools grows up to its scroll limit', (
      tester,
    ) async {
      final transcript = fixture.Transcript();
      for (var index = 0; index < count; index++) {
        transcript.tool('tool-$index');
      }
      transcript.text(
        'answer',
        'Final answer\n\n${List.filled(60, 'More text').join('\n\n')}',
      );
      final projection = ValueNotifier(transcript.view());
      addTearDown(projection.dispose);
      await fixture.mount(tester, projection);
      await fixture.tap(tester, find.text(tr('completed')).first);
      await fixture.tap(tester, find.text('tool-${count - 1}').first);
      final group = find.byType(ToolSequence);
      final scroll = find.descendant(
        of: group,
        matching: find.byWidgetPredicate(
          (widget) =>
              widget is SingleChildScrollView && widget.controller != null,
        ),
      );
      expect(scroll, findsOneWidget);
      final controller = tester
          .widget<SingleChildScrollView>(scroll)
          .controller!;
      final viewport = tester.getRect(scroll);
      expect(viewport.height, lessThanOrEqualTo(320));
      final outer = Scrollable.of(tester.element(group)).position;
      if (count == 2) {
        expect(viewport.height, lessThan(320));
        expect(controller.position.maxScrollExtent, 0);
      } else {
        expect(viewport.height, 320);
        expect(controller.position.maxScrollExtent, greaterThan(0));
        final before = outer.pixels;
        await tester.dragFrom(viewport.center, const Offset(0, -160));
        await tester.pumpAndSettle();
        expect(controller.offset, greaterThan(0));
        expect(outer.pixels, before);
        expect(tester.getRect(scroll), viewport);

        tester.view.physicalSize = const Size(420, 500);
        await tester.pumpAndSettle();
        expect(tester.getSize(scroll).height, 250);
        await tester.ensureVisible(find.text('Final answer'));
        final outsideBefore = outer.pixels;
        await tester.drag(find.text('Final answer'), const Offset(0, -100));
        await tester.pumpAndSettle();
        expect(outer.pixels, greaterThan(outsideBefore));
      }
      expect(tester.takeException(), isNull);
    });
  }
}
