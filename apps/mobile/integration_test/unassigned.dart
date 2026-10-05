import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/app.dart';
import 'package:sailry_mobile/features/conversations/live/create.dart';
import 'package:sailry_mobile/features/conversations/live/page.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/ui/kit.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'input.dart';

Future<void> conversationWithoutProject(
  WidgetTester tester, {
  required String invitation,
  required String profile,
  required bool internet,
  required bool nativeIme,
  required Future<void> Function() nativeInput,
  required Future<void> Function(WidgetTester, bool Function()) until,
}) async {
  final app = AppSession();
  try {
    // The isolated UI fixture exercises input, not notification permissions.
    await app.background.setEnabled(false);
    await app.start(path: profile, internet: internet);
    expect(app.error, isNull);
    await app.pair(invitation);
    await tester.pumpWidget(SailryApp(session: app));
    await until(
      tester,
      () =>
          app.hosts.single.connected &&
          objects(app.hosts.single.snapshot['providers']).isNotEmpty,
    );
    final node = app.hosts.single;
    expect(objects(node.snapshot['projects']), isEmpty);
    await tester.tap(find.byTooltip(tr('newConversation')));
    await tester.pumpAndSettle();
    expect(find.byType(NewConversationPage), findsOneWidget);
    expect(find.text(tr('conversationNoProject')), findsNothing);
    await edit(tester, find.byType(TextField), 'Hello from mobile');
    await tester.tap(find.byTooltip(tr('send')));
    await until(
      tester,
      () => find.text('answer-flutter-fixture').evaluate().isNotEmpty,
    );
    expect(find.text('Hello from mobile'), findsWidgets);
    expect(objects(node.snapshot['projects']), isEmpty);
    final conversation = objects(node.snapshot['sessions']).single;
    expect(conversation['project'], isNull);
    final worktree = objects(node.snapshot['worktrees']).single;
    expect(conversation['worktree'], worktree['id']);

    final created = object(
      (await node.command('create_terminal', {
        'worktree': conversation['worktree'],
        ...terminalLaunch(tester.element(find.byType(LiveConversationPage))),
      }))['data'],
    );
    // The Rust fixture takes control as the execution Node, distinct from this phone.
    await until(
      tester,
      () => objects(node.snapshot['terminals']).any(
        (terminal) =>
            terminal['id'] == created['id'] &&
            number(terminal['revision']) > number(created['revision']),
      ),
    );
    await tester.pumpWidget(
      SessionScope(
        session: app,
        child: MaterialApp(
          theme: SailryTheme.of(Brightness.dark),
          home: TerminalPage(hostId: node.id, terminalId: text(created['id'])),
        ),
      ),
    );
    final control = find.widgetWithText(
      FilledButton,
      tr('resourceTerminalControl'),
    );
    await until(tester, () => control.evaluate().isNotEmpty);
    expect(find.byType(EmptyState), findsOneWidget);
    await tester.tap(control);
    final editor = find.byType(EditableText);
    await until(
      tester,
      () =>
          editor.evaluate().isNotEmpty &&
          !tester.widget<EditableText>(editor).readOnly,
    );
    expect(
      find.byKey(const ValueKey('terminal-control-overlay')),
      findsNothing,
    );
    final input = tester.widget<EditableText>(editor);
    expect(input.keyboardType, TextInputType.text);
    expect(input.enableSuggestions, isTrue);
    expect(input.obscureText, isFalse);
    expect(input.autocorrect, isFalse);
    final state = tester.state<EditableTextState>(editor);
    state.updateEditingValue(
      const TextEditingValue(
        text: 'zhong',
        selection: TextSelection.collapsed(offset: 5),
        composing: TextRange(start: 0, end: 5),
      ),
    );
    await tester.pump();
    expect(
      tester
          .widget<Opacity>(find.byKey(const ValueKey('terminal-ime')))
          .opacity,
      1,
    );
    expectCompositionFits(tester, editor);
    final grid = find.byWidgetPredicate(
      (widget) =>
          widget is CustomPaint && widget.painter is TerminalGridPainter,
    );
    final bounds = tester.getRect(grid);
    final inputBounds = tester.getRect(editor);
    expect(inputBounds.left, greaterThanOrEqualTo(bounds.left));
    expect(inputBounds.right, lessThanOrEqualTo(bounds.right));
    String output() {
      final painter =
          tester
                  .widget<CustomPaint>(
                    find.byWidgetPredicate(
                      (widget) =>
                          widget is CustomPaint &&
                          widget.painter is TerminalGridPainter,
                    ),
                  )
                  .painter!
              as TerminalGridPainter;
      return terminalLines(painter.screen)
          .expand((line) => objects(line['spans']))
          .map((span) => text(span['text']))
          .join();
    }

    final echo = Stopwatch()..start();
    final binding = tester.binding;
    final previousPolicy = binding is LiveTestWidgetsFlutterBinding
        ? binding.framePolicy
        : null;
    if (nativeIme && binding is LiveTestWidgetsFlutterBinding) {
      binding.framePolicy = LiveTestWidgetsFlutterBindingFramePolicy.fullyLive;
    }
    state.updateEditingValue(
      const TextEditingValue(
        text: '中',
        selection: TextSelection.collapsed(offset: 1),
      ),
    );
    Future<void> echoUntil(String value) async {
      final deadline = DateTime.now().add(const Duration(seconds: 10));
      while (!output().contains(value)) {
        if (DateTime.now().isAfter(deadline)) {
          fail('Terminal echo deadline');
        }
        if (nativeIme) {
          // Observe natural app frames, rather than gating them behind test pumps.
          await Future<void>.delayed(const Duration(milliseconds: 1));
        } else {
          await tester.pump(const Duration(milliseconds: 1));
        }
      }
    }

    await echoUntil('中');
    echo.stop();
    debugPrint('Terminal committed-text echo: ${echo.elapsedMilliseconds} ms');
    expect(
      tester
          .widget<Opacity>(find.byKey(const ValueKey('terminal-ime')))
          .opacity,
      0,
    );
    final samples = <int>[];
    final calls = <int>[];
    for (var index = 0; index < 20; index++) {
      final elapsed = Stopwatch()..start();
      await node.command('input_terminal', {
        'terminal': created['id'],
        'revision': number(
          objects(node.snapshot['terminals']).single['revision'],
        ).toInt(),
        'input': {
          'focus': {'focused': true},
        },
      });
      calls.add(elapsed.elapsedMicroseconds);
    }
    calls.sort();
    debugPrint(
      'Terminal command completion, 20 samples: '
      'p50=${calls[9] / 1000} ms, p95=${calls[18] / 1000} ms, '
      'max=${calls.last / 1000} ms',
    );
    for (var index = 0; index < 20; index++) {
      final marker = 'probe${index}x';
      final elapsed = Stopwatch()..start();
      state.updateEditingValue(
        TextEditingValue(
          text: marker,
          selection: TextSelection.collapsed(offset: marker.length),
        ),
      );
      await echoUntil(marker);
      samples.add(elapsed.elapsedMicroseconds);
    }
    samples.sort();
    debugPrint(
      'Terminal UI echo, 20 samples, 1 ms observation interval: '
      'p50=${samples[9] / 1000} ms, p95=${samples[18] / 1000} ms, '
      'max=${samples.last / 1000} ms',
    );
    if (previousPolicy != null && binding is LiveTestWidgetsFlutterBinding) {
      binding.framePolicy = previousPolicy;
    }
    if (nativeIme) {
      state.requestKeyboard();
      await tester.pump();
      debugPrint('SAILRY_ANDROID_IME_READY');
      await nativeInput();
      // The connected device's actual keyboard/input connection supplies this marker.
      await until(tester, () => output().contains('sailryime'));
      debugPrint('SAILRY_ANDROID_IME_INPUT_RECEIVED');
    }
    expect(tester.takeException(), isNull);
  } finally {
    await tester.pumpWidget(const SizedBox());
    await app.close();
    app.dispose();
  }
}
