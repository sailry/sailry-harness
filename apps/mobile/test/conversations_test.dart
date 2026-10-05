import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_slidable/flutter_slidable.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/conversation_page.dart';
import 'package:sailry_mobile/features/conversations/tasks_page.dart';
import 'package:sailry_mobile/features/conversations/running_task_frame.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/theme.dart';

Future<void> mount(
  WidgetTester tester,
  Widget page, {
  Size size = const Size(390, 844),
  Brightness brightness = Brightness.light,
  double textScale = 1,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    MaterialApp(
      theme: SailryTheme.of(brightness),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context).copyWith(
          textScaler: TextScaler.linear(textScale),
          disableAnimations: true,
        ),
        child: child!,
      ),
      home: page,
    ),
  );
  await tester.pumpAndSettle();
}

Future<void> demo(WidgetTester tester, String label) async {
  await tester.tap(find.byTooltip(tr('more')));
  await tester.pumpAndSettle();
  await tester.tap(find.text(tr('replyPreview')));
  await tester.pumpAndSettle();
  await tester.ensureVisible(find.text(label));
  await tester.tap(find.text(label));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('running border animates only while visible', (tester) async {
    var visible = true;
    var reduced = false;
    late StateSetter update;
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: StatefulBuilder(
          builder: (context, setState) {
            update = setState;
            return MediaQuery(
              data: MediaQuery.of(context).copyWith(disableAnimations: reduced),
              child: TickerMode(
                enabled: visible,
                child: const Center(
                  child: SizedBox(
                    width: 280,
                    height: 100,
                    child: RunningTaskFrame(
                      active: true,
                      child: SizedBox.expand(),
                    ),
                  ),
                ),
              ),
            );
          },
        ),
      ),
    );
    final first = tester.widget<ShaderMask>(find.byType(ShaderMask));
    await tester.pump(const Duration(milliseconds: 500));
    final moving = tester.widget<ShaderMask>(find.byType(ShaderMask));
    expect(identical(first, moving), isFalse);
    update(() => visible = false);
    await tester.pump();
    final hidden = tester.widget<ShaderMask>(find.byType(ShaderMask));
    await tester.pump(const Duration(seconds: 2));
    expect(
      identical(hidden, tester.widget<ShaderMask>(find.byType(ShaderMask))),
      isTrue,
    );
    update(() {
      visible = true;
      reduced = true;
    });
    await tester.pumpAndSettle();
    final still = tester.widget<ShaderMask>(find.byType(ShaderMask));
    await tester.pump(const Duration(seconds: 2));
    expect(
      identical(still, tester.widget<ShaderMask>(find.byType(ShaderMask))),
      isTrue,
    );
    expect(tester.takeException(), isNull);
  });

  group('Task list', () {
    testWidgets('filter labels fit at larger text sizes', (tester) async {
      for (final scale in [1.0, 1.8]) {
        await mount(
          tester,
          const TasksPage(),
          size: const Size(320, 640),
          textScale: scale,
        );
        for (final filter in [
          'all',
          'waiting',
          'running',
          'completed',
          'archived',
        ]) {
          final button = find.byKey(ValueKey('task-filter-$filter'));
          final label = find.descendant(
            of: button,
            matching: find.byType(Text),
          );
          final paragraph = tester.renderObject<RenderParagraph>(label);
          expect(
            paragraph.size.width,
            greaterThanOrEqualTo(
              paragraph.getMaxIntrinsicWidth(double.infinity) - .1,
            ),
          );
          expect(paragraph.didExceedMaxLines, isFalse);
          expect(tester.getSize(button).height, greaterThanOrEqualTo(44));
        }
        final archive = find.byKey(const ValueKey('task-filter-archived'));
        await tester.ensureVisible(archive);
        await tester.pumpAndSettle();
        await tester.tap(archive);
        await tester.pumpAndSettle();
        expect(find.text(tr('archiveEmpty')), findsOneWidget);
        expect(tester.takeException(), isNull);
        await tester.pumpWidget(const SizedBox.shrink());
      }
    });

    testWidgets('filters by status and project', (tester) async {
      await mount(tester, const TasksPage());
      await tester.tap(find.text(tr('waiting')));
      await tester.pumpAndSettle();
      expect(find.text(tr('approveTitle')), findsOneWidget);
      expect(find.text(tr('taskSearch')), findsNothing);
      await tester.tap(find.text(tr('allProjects')));
      await tester.pumpAndSettle();
      await tester.tap(find.text('sailry-api'));
      await tester.pumpAndSettle();
      expect(find.text(tr('questionTitle')), findsOneWidget);
      expect(find.text(tr('approveTitle')), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('creates an isolated draft', (tester) async {
      await mount(tester, const TasksPage());
      await tester.tap(find.byTooltip(tr('newTask')));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.widgetWithText(TextField, tr('describeTask')),
        'Review spacing',
      );
      await tester.ensureVisible(find.text(tr('create')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('create')));
      await tester.pumpAndSettle();
      expect(find.byType(ConversationPage), findsOneWidget);
      expect(find.text('Review spacing'), findsWidgets);
      expect(find.text(tr('allowShort')), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('slide to archive and restore', (tester) async {
      await mount(tester, const TasksPage());
      await tester.drag(
        find.byKey(const ValueKey('approval')),
        const Offset(-240, 0),
      );
      await tester.pumpAndSettle();
      await tester.tap(
        find.widgetWithText(CustomSlidableAction, tr('archiveShort')).first,
      );
      await tester.pumpAndSettle();
      expect(find.text(tr('approveTitle')), findsNothing);
      await tester.tap(find.text(tr('archiveTab')));
      await tester.pumpAndSettle();
      expect(find.text(tr('approveTitle')), findsOneWidget);
      await tester.drag(
        find.byKey(const ValueKey('approval')),
        const Offset(-240, 0),
      );
      await tester.pumpAndSettle();
      await tester.tap(
        find.widgetWithText(CustomSlidableAction, tr('restore')),
      );
      await tester.pumpAndSettle();
      expect(find.text(tr('archiveEmpty')), findsOneWidget);
    });
  });

  group('Conversation', () {
    testWidgets('single priority strip and queue editing', (tester) async {
      await mount(tester, const ConversationPage());
      expect(find.byKey(const ValueKey('composer-priority')), findsOneWidget);
      expect(find.text(tr('allowShort')), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'Review focus states');
      await tester.tap(find.byTooltip(tr('enqueue')));
      await tester.pumpAndSettle();
      expect(find.text(tr('allowShort')), findsOneWidget);
      await tester.tap(find.text(tr('deny')));
      await tester.pumpAndSettle();
      expect(find.text('${tr('queueShort')}  2'), findsOneWidget);
      await tester.tap(find.text('${tr('queueShort')}  2'));
      await tester.pumpAndSettle();
      expect(find.text('Review focus states'), findsOneWidget);
      await tester.tap(find.text('Review focus states'));
      await tester.pumpAndSettle();
      await tester.enterText(
        find.byType(TextField).last,
        'Review keyboard focus',
      );
      await tester.tap(find.text(tr('save')));
      await tester.pumpAndSettle();
      expect(find.text('Review keyboard focus'), findsOneWidget);
      expect(find.text('Review focus states'), findsNothing);
      expect(tester.takeException(), isNull);
    });

    testWidgets('streaming preserves the draft', (tester) async {
      await mount(tester, const ConversationPage());
      await demo(tester, tr('phaseThinking'));
      await tester.enterText(find.byType(TextField), 'Keep this draft');
      await tester.tap(find.byTooltip(tr('more')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('replyPreview')));
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('playFlow')));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.pump(const Duration(seconds: 1));
      expect(find.text('Keep this draft'), findsOneWidget);
      expect(find.byType(CircularProgressIndicator), findsWidgets);
      await tester.tap(find.byTooltip(tr('stop')));
      await tester.pumpAndSettle();
      expect(find.byType(CircularProgressIndicator), findsNothing);
      expect(find.text('Keep this draft'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    testWidgets('question and failed tool remain actionable', (tester) async {
      await mount(tester, const ConversationPage());
      await demo(tester, tr('phaseQuestion'));
      await tester.tap(find.widgetWithText(FilledButton, tr('reply')));
      await tester.pumpAndSettle();
      expect(find.text(tr('wideButton')), findsOneWidget);
      await tester.tap(find.text(tr('wideButton')));
      await tester.pump(const Duration(milliseconds: 400));
      await tester.tap(find.byTooltip(tr('stop')));
      await tester.pumpAndSettle();
      await demo(tester, tr('phaseFailed'));
      await tester.ensureVisible(find.text(tr('retryTask')).last);
      await tester.pumpAndSettle();
      await tester.tap(find.text(tr('retryTask')).last);
      await tester.pump(const Duration(milliseconds: 400));
      expect(find.byType(CircularProgressIndicator), findsWidgets);
      await tester.tap(find.byTooltip(tr('stop')));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    });

    testWidgets('queued followup result remains visible', (tester) async {
      await mount(tester, const ConversationPage());
      await demo(tester, tr('phaseComplete'));
      expect(find.text(tr('flowResult')), findsOneWidget);
      expect(find.text(tr('queueSample')), findsOneWidget);
      await tester.scrollUntilVisible(
        find.text(tr('followupResult')),
        250,
        scrollable: find
            .descendant(
              of: find.byType(ListView),
              matching: find.byType(Scrollable),
            )
            .first,
      );
      await tester.pumpAndSettle();
      expect(find.text(tr('followupResult')), findsOneWidget);
      expect(find.text(tr('completed')), findsOneWidget);
      expect(tester.takeException(), isNull);
    });

    for (final brightness in Brightness.values) {
      testWidgets('fits narrow viewport in ${brightness.name}', (tester) async {
        await mount(
          tester,
          const ConversationPage(),
          size: const Size(320, 640),
          brightness: brightness,
        );
        final input = tester.getRect(find.byType(TextField));
        final send = tester.getRect(find.byTooltip(tr('enqueue')));
        // The compact composer shares its first line with the action buttons.
        expect(input.width, greaterThanOrEqualTo(120));
        expect(input.right, lessThan(send.left));
        expect(send.right, lessThanOrEqualTo(320));
        expect(send.bottom, lessThan(640));
        expect(tester.takeException(), isNull);
      });
    }
  });
}
