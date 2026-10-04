import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/features/conversations/conversation_page.dart';
import 'package:sailry_mobile/features/conversations/message_composer.dart';
import 'package:sailry_mobile/features/conversations/live/attachments.dart';
import 'package:sailry_mobile/features/conversations/live/draft_attachments.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/ui/theme.dart';

void main() {
  testWidgets('stacked attachments preserve width', (tester) async {
    tester.view.physicalSize = const Size(320, 740);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final controller = TextEditingController();
    final attachments = <PickedAttachment>[
      for (final name in [
        'Screenshot_with_a_very_long_name_that_must_be_truncated.png',
        'notes.txt',
      ])
        (
          attachment: {
            'spec': {'name': name},
          },
          preview: null,
        ),
    ];
    await tester.pumpWidget(
      MaterialApp(
        theme: SailryTheme.of(Brightness.dark),
        home: StatefulBuilder(
          builder: (context, update) => Scaffold(
            body: Align(
              alignment: Alignment.bottomCenter,
              child: MessageComposer(
                controller: controller,
                attached: false,
                busy: false,
                onAttachment: (_) {},
                onVoice: () {},
                onStop: () {},
                onSend: () {},
                attachments: DraftAttachments(
                  attachments: attachments,
                  onRemove: (item) => update(() => attachments.remove(item)),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    final strip = tester.getRect(find.byType(DraftAttachments));
    final input = tester.getRect(find.byKey(const ValueKey('composer-input')));
    expect(strip.bottom, input.top);
    final name = find.textContaining('Screenshot_');
    expect(tester.widget<Text>(name).maxLines, 1);
    expect(tester.widget<Text>(name).overflow, TextOverflow.ellipsis);
    expect(
      tester.getRect(name).bottom,
      lessThan(tester.getRect(find.text('notes.txt')).top),
    );
    await tester.tap(find.byTooltip(tr('removeAttachment')).first);
    await tester.pumpAndSettle();
    expect(name, findsNothing);
    expect(find.text('notes.txt'), findsOneWidget);
    expect(
      tester.getRect(find.byKey(const ValueKey('composer-input'))).bottom,
      input.bottom,
    );
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    controller.dispose();
  });

  testWidgets('priority, input, and actions fit', (tester) async {
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    for (final width in [320.0, 390.0]) {
      tester.view.physicalSize = Size(width, 844);
      for (final density in [VisualDensity.standard, VisualDensity.compact]) {
        for (final scale in [1.0, 2.0]) {
          final theme = SailryTheme.of(
            Brightness.dark,
          ).copyWith(visualDensity: density);
          Widget page = const ConversationPage();
          TextEditingController? controller;
          if (scale > 1) {
            await tester.pumpWidget(MaterialApp(theme: theme, home: page));
            await tester.pumpAndSettle();
            expect(tester.takeException(), isNull);
            final composer = tester.widget<MessageComposer>(
              find.byType(MessageComposer),
            );
            controller = TextEditingController();
            // Exercise the actual priority strip independently of the transcript.
            page = Scaffold(
              body: Align(
                alignment: Alignment.bottomCenter,
                child: MessageComposer(
                  controller: controller,
                  attached: composer.attached,
                  busy: composer.busy,
                  priority: composer.priority,
                  onAttachment: (_) {},
                  onVoice: () {},
                  onStop: () {},
                  onSend: () {},
                ),
              ),
            );
          }
          await tester.pumpWidget(
            MaterialApp(
              theme: theme,
              builder: (context, child) => MediaQuery(
                data: MediaQuery.of(context).copyWith(
                  textScaler: TextScaler.linear(scale),
                  disableAnimations: true,
                ),
                child: child!,
              ),
              home: page,
            ),
          );
          await tester.pumpAndSettle();
          final priority = find.byKey(const ValueKey('composer-priority'));
          final material = find
              .descendant(of: priority, matching: find.byType(Material))
              .first;
          final strip = tester.getRect(material);
          final input = tester.getRect(
            find.byKey(const ValueKey('composer-input')),
          );
          expect(strip.bottom, closeTo(input.top, .01));
          expect(strip.left, greaterThan(input.left));
          expect(strip.right, lessThan(input.right));
          expect(
            tester.widget<Material>(material).clipBehavior,
            Clip.antiAlias,
          );
          expect(
            find.descendant(of: priority, matching: find.text(tr('approval'))),
            findsNothing,
          );
          expect(
            find.descendant(of: priority, matching: find.text('pnpm test')),
            findsOneWidget,
          );
          final actionHeights = <double>[];
          for (final (type, label) in [
            (TextButton, tr('deny')),
            (FilledButton, tr('allowShort')),
          ]) {
            final button = find.descendant(
              of: priority,
              matching: find.widgetWithText(type, label),
            );
            final bounds = tester.getRect(button);
            actionHeights.add(bounds.height);
            final text = tester.getRect(
              find.descendant(of: button, matching: find.text(label)),
            );
            expect(bounds.height, greaterThanOrEqualTo(32));
            expect(text.top - bounds.top, greaterThanOrEqualTo(4.9));
            expect(bounds.bottom - text.bottom, greaterThanOrEqualTo(4.9));
            expect(text.left, greaterThanOrEqualTo(bounds.left));
            expect(text.right, lessThanOrEqualTo(bounds.right));
          }
          expect(actionHeights[0], closeTo(actionHeights[1], .01));
          if (scale == 1) expect(actionHeights[0], 32);
          final send = tester.getRect(find.byTooltip(tr('enqueue')));
          expect(input.right - send.right, closeTo(8, .01));
          expect(tester.takeException(), isNull);
          await tester.pumpWidget(const SizedBox.shrink());
          controller?.dispose();
        }
      }
    }
  });
}
