import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/live/glass_chip.dart';
import 'package:sailry_mobile/features/conversations/live/progress.dart';
import 'package:sailry_mobile/features/conversations/live/queue.dart';
import 'package:sailry_mobile/l10n/strings.dart';

Future<void> narrow(WidgetTester tester, Widget child) async {
  tester.view.physicalSize = const Size(320, 640);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(MaterialApp(home: Scaffold(body: child)));
}

void main() {
  testWidgets('composer chips share one scrolling line on narrow screens', (
    tester,
  ) async {
    var opened = 0;
    await narrow(
      tester,
      Align(
        alignment: Alignment.bottomCenter,
        child: ComposerChipRow(
          children: [
            QueueChip(
              key: const ValueKey('queue'),
              state: (count: 3, paused: false, guiding: true),
              onOpen: () => opened++,
            ),
            for (final label in [
              'Working tree changes +120 -48',
              'Background tasks running',
              'Progress 2/5',
            ])
              GlassChip(key: ValueKey(label), onTap: () {}, child: Text(label)),
          ],
        ),
      ),
    );
    expect(tester.takeException(), isNull);
    final chips = [
      find.byKey(const ValueKey('queue')),
      find.byKey(const ValueKey('Working tree changes +120 -48')),
      find.byKey(const ValueKey('Background tasks running')),
      find.byKey(const ValueKey('Progress 2/5')),
    ];
    final top = tester.getTopLeft(chips.first).dy;
    for (final chip in chips) {
      expect(tester.getTopLeft(chip).dy, top);
      expect(tester.getSize(chip).height, 32);
    }
    final scroll = tester.widget<SingleChildScrollView>(
      find.byKey(const ValueKey('composer-chips')),
    );
    expect(scroll.scrollDirection, Axis.horizontal);
    final position = tester
        .state<ScrollableState>(find.byType(Scrollable))
        .position;
    expect(position.maxScrollExtent, greaterThan(0));

    await tester.tap(chips.first);
    expect(opened, 1);
  });

  testWidgets('progress steps take one line with their state as an icon', (
    tester,
  ) async {
    final semantics = tester.ensureSemantics();
    const long =
        'Trace every caller of the shared projection before changing the '
        'stored format and update each reader';
    await narrow(
      tester,
      const TaskProgress(
        progress: {
          'steps': [
            {'description': long, 'state': 'completed'},
            {'description': 'Run', 'state': 'in_progress'},
            {'description': 'Ship', 'state': 'pending'},
            {'description': 'Skip', 'state': 'skipped'},
          ],
        },
      ),
    );
    expect(tester.takeException(), isNull);
    for (final key in [
      'completed',
      'running',
      'conversationToolWaiting',
      'conversationStepSkipped',
    ]) {
      expect(find.text(tr(key)), findsNothing);
    }
    final first = tester.getSize(find.byKey(const ValueKey('progress-step-0')));
    final short = tester.getSize(find.byKey(const ValueKey('progress-step-2')));
    expect(first.height, short.height);
    expect(find.bySemanticsLabel('$long, ${tr('completed')}'), findsOneWidget);
    expect(find.bySemanticsLabel('Run, ${tr('running')}'), findsOneWidget);
    expect(
      find.bySemanticsLabel('Ship, ${tr('conversationToolWaiting')}'),
      findsOneWidget,
    );
    expect(
      find.bySemanticsLabel('Skip, ${tr('conversationStepSkipped')}'),
      findsOneWidget,
    );
    semantics.dispose();
  });
}
