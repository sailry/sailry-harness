import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_bridge/api/connection.dart';
import 'package:sailry_bridge/api/terminal.dart';
import 'package:sailry_mobile/features/terminal/appearance.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/features/terminal/toolbar.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/runtime/json.dart';
import 'package:sailry_mobile/ui/theme.dart';
import 'package:sailry_mobile/ui/kit.dart';
import '../integration_test/input.dart';

class UpdatesFixture implements TerminalUpdates {
  final _values = StreamController<String>();
  late final _iterator = StreamIterator(_values.stream);
  bool closed = false;
  bool released = false;
  void emit(Map<String, dynamic> value) => _values.add(jsonEncode(value));
  @override
  Future<String> next() async {
    if (await _iterator.moveNext()) return _iterator.current;
    throw StateError('closed');
  }

  @override
  Future<void> close() async {
    if (closed) return;
    closed = true;
    unawaited(_values.close());
  }

  @override
  void dispose() => released = true;
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class ConnectionFixture implements Connection {
  final updates = UpdatesFixture();
  final watched = <String>[];
  final retried = <String>[];
  @override
  Future<String> execute({required String request}) async {
    retried.add(request);
    return jsonEncode({
      'Ok': {'kind': 'terminal', 'data': info(3)},
    });
  }

  @override
  Future<TerminalUpdates> watchTerminal({required String terminal}) async {
    watched.add(terminal);
    return updates;
  }

  @override
  Future<void> close() => updates.close();
  @override
  void dispose() {}
  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Map<String, dynamic> info(int revision) => {
  'id': 'terminal',
  'revision': revision,
  'status': {'kind': 'running'},
};

Map<String, dynamic> view(int revision, {bool connected = true}) => {
  'connected': connected,
  'snapshot': {
    'info': info(revision),
    'screen': {
      'columns': 30,
      'foreground': {'red': 32, 'green': 32, 'blue': 32},
      'background': {'red': 250, 'green': 250, 'blue': 250},
      'scrollback': [],
      'rows': [
        {
          'spans': [
            {'column': 0, 'columns': 7, 'text': 'Actual ', 'style': {}},
          ],
        },
        {
          'spans': [
            {'column': 0, 'columns': 2, 'text': '中', 'style': {}},
          ],
        },
      ],
    },
  },
};

Future<void> mount(WidgetTester tester, AppSession session) async {
  tester.view.physicalSize = const Size(430, 900);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    SessionScope(
      session: session,
      child: MaterialApp(
        theme: SailryTheme.of(Brightness.light),
        home: const TerminalPage(hostId: 'node', terminalId: 'terminal'),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  for (final fail in [false, true]) {
    testWidgets(
      'queued text ${fail ? 'drops after failure' : 'batches across a slow response'}',
      (tester) async {
        final connection = ConnectionFixture();
        final commands = <(String, Map<String, dynamic>?)>[];
        final held = Completer<Map<String, dynamic>>();
        final host = HostConnection.test(
          id: 'node',
          label: 'Host',
          connection: connection,
          command: (kind, data) async {
            commands.add((kind, data));
            if (kind == 'claim_terminal') return {'data': info(2)};
            if (object(object(data?['input'])['text'])['text'] == 'a') {
              return held.future;
            }
            return {'data': {}};
          },
        );
        final session = AppSession.test(hosts: [host]);
        connection.updates.emit(view(2));
        await mount(tester, session);
        await tester.tap(
          find.widgetWithText(FilledButton, tr('resourceTerminalControl')),
        );
        await tester.pumpAndSettle();
        commands.clear();
        void type(String value) => tester.testTextInput.updateEditingValue(
          TextEditingValue(
            text: value,
            selection: TextSelection.collapsed(offset: value.length),
          ),
        );
        type('a');
        await tester.pump();
        type('b');
        type('c');
        type('中');
        final enter = find.byTooltip(tr('terminalEnter'));
        await tester.ensureVisible(enter);
        await tester.tap(enter);
        type('d');
        type('e');
        await tester.pump();
        expect(commands, hasLength(1));
        if (fail) {
          held.completeError(const CommandFailure('outcome_unknown'));
        } else {
          held.complete({'data': {}});
        }
        await tester.pumpAndSettle();
        if (fail) {
          expect(commands, hasLength(1));
          expect(
            find.byKey(const ValueKey('terminal-control-overlay')),
            findsOneWidget,
          );
          expect(
            tester.widget<EditableText>(find.byType(EditableText)).readOnly,
            isTrue,
          );
          await tester.pump(const Duration(seconds: 3));
        } else {
          final inputs = commands
              .map((command) => command.$2!['input'])
              .toList();
          expect(inputs, hasLength(4));
          expect(inputs[0], {
            'text': {'text': 'a'},
          });
          expect(inputs[1], {
            'text': {'text': 'bc中'},
          });
          expect(inputs[2]['key']['event']['key'], 'enter');
          expect(inputs[3], {
            'text': {'text': 'de'},
          });
        }
        await tester.pumpWidget(const SizedBox());
        await session.close();
        session.dispose();
      },
    );
  }

  testWidgets('uncertain reopen preserves the terminal and request', (
    tester,
  ) async {
    final connection = ConnectionFixture();
    final commands = <String>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      connection: connection,
      command: (kind, data) async {
        commands.add(kind);
        expect(data!['terminal'], 'terminal');
        throw const CommandFailure('outcome_unknown', request: 'open-request');
      },
    );
    final session = AppSession.test(hosts: [host]);
    await mount(tester, session);
    expect(find.byType(FailureState), findsOneWidget);
    expect(connection.watched, isEmpty);
    expect(commands, ['open_terminal']);
    connection.updates.emit(view(3));
    await tester.tap(find.text(tr('retry')));
    await tester.pumpAndSettle();
    expect(connection.retried, ['open-request']);
    expect(commands, ['open_terminal']);
    expect(connection.watched, ['terminal']);
    expect(find.byType(FailureState), findsNothing);
    expect(find.text(tr('resourceTerminalControlHint')), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });

  testWidgets('subscription failure and view recovery', (tester) async {
    final connection = ConnectionFixture();
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      connection: connection,
      command: (_, _) async => {'data': {}},
    );
    final session = AppSession.test(hosts: [host]);
    connection.updates.emit({
      ...view(1, connected: false),
      'error': {'message': 'Connection lost'},
    });
    await mount(tester, session);
    expect(find.byType(FailureState), findsOneWidget);
    final bounds = tester.getRect(find.byType(FailureState));
    expect(bounds.height, greaterThan(600));
    expect(
      tester.getCenter(find.text('Connection lost')).dy,
      greaterThan(bounds.center.dy),
    );
    expect(find.byType(EditableText), findsNothing);
    connection.updates.emit(view(1));
    await tester.pumpAndSettle();
    expect(find.byType(FailureState), findsNothing);
    expect(find.text(tr('resourceTerminalControlHint')), findsOneWidget);
    expect(connection.watched, ['terminal']);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await session.close();
    session.dispose();
  });

  testWidgets('focused cursor respects Node modes', (tester) async {
    final connection = ConnectionFixture();
    final commands = <String>[];
    Map<String, dynamic> screen({
      int column = 1,
      bool blinking = true,
      bool hidden = false,
    }) {
      final state = view(2);
      state['snapshot']['screen'] = <String, dynamic>{
        ...state['snapshot']['screen'],
        'cursor': hidden
            ? null
            : {
                'row': 0,
                'column': column,
                'style': 'bar',
                'blinking': blinking,
              },
      };
      return state;
    }

    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      connection: connection,
      command: (kind, _) async {
        commands.add(kind);
        return kind == 'claim_terminal' ? {'data': info(2)} : {'data': {}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    connection.updates.emit(screen());
    await mount(tester, session);
    TerminalGridPainter painter() =>
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
    expect(painter().focused, isFalse);
    expect(painter().cursorVisible, isTrue);
    await tester.tap(
      find.widgetWithText(FilledButton, tr('resourceTerminalControl')),
    );
    await tester.pump();
    await tester.pump();
    expect(painter().focused, isTrue);
    expect(painter().cursorVisible, isTrue);
    await tester.pump(const Duration(milliseconds: 540));
    expect(painter().cursorVisible, isFalse);
    connection.updates.emit(screen());
    await tester.pump();
    expect(
      painter().cursorVisible,
      isFalse,
      reason: 'background output must not reset blinking',
    );
    final count = commands.length;
    await tester.pump(const Duration(milliseconds: 530));
    expect(painter().cursorVisible, isTrue);
    expect(commands.length, count, reason: 'blinking is local paint state');
    await tester.pump(const Duration(milliseconds: 530));
    connection.updates.emit(screen(column: 2));
    await tester.pump();
    expect(painter().cursorVisible, isTrue);
    await tester.pump(const Duration(milliseconds: 540));
    expect(painter().cursorVisible, isFalse);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    await tester.pump();
    expect(painter().focused, isFalse);
    expect(painter().cursorVisible, isTrue);
    await tester.pump(const Duration(seconds: 1));
    expect(painter().cursorVisible, isTrue);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pump();
    expect(painter().focused, isTrue);
    connection.updates.emit(screen(blinking: false));
    await tester.pump();
    await tester.pump(const Duration(seconds: 1));
    expect(painter().cursorVisible, isTrue);
    connection.updates.emit(screen(hidden: true));
    await tester.pump();
    await tester.pump();
    expect(painter().screen['cursor'], isNull);
    expect(painter().cursorVisible, isFalse);
    connection.updates.emit(screen());
    await tester.pump();
    tester.widget<EditableText>(find.byType(EditableText)).focusNode.unfocus();
    await tester.pump();
    expect(painter().focused, isFalse);
    await tester.pump(const Duration(seconds: 1));
    expect(painter().cursorVisible, isTrue);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    session.dispose();
  });

  testWidgets('composition stays visible at the right edge', (tester) async {
    final connection = ConnectionFixture();
    final commands = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      connection: connection,
      command: (kind, data) async {
        commands.add((kind, data));
        return kind == 'claim_terminal' ? {'data': info(2)} : {'data': {}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    final state = view(2);
    state['snapshot']['screen']['cursor'] = {'row': 0, 'column': 29};
    connection.updates.emit(state);
    await mount(tester, session);
    await tester.tap(
      find.widgetWithText(FilledButton, tr('resourceTerminalControl')),
    );
    await tester.pumpAndSettle();
    final editor = find.byType(EditableText);
    final grid = find.byWidgetPredicate(
      (widget) =>
          widget is CustomPaint && widget.painter is TerminalGridPainter,
    );
    final bounds = tester.getRect(grid);
    final painter =
        tester.widget<CustomPaint>(grid).painter! as TerminalGridPainter;
    for (final text in ['zhong', 'zhongwen']) {
      tester.testTextInput.updateEditingValue(
        TextEditingValue(
          text: text,
          selection: TextSelection.collapsed(offset: text.length),
          composing: TextRange(start: 0, end: text.length),
        ),
      );
      await tester.pump();
      expectCompositionFits(tester, editor);
      final input = tester.getRect(editor);
      expect(input.left, lessThan(bounds.left + 29 * painter.cell.width));
      expect(input.left, greaterThanOrEqualTo(bounds.left));
      expect(input.right, lessThanOrEqualTo(bounds.right));
    }
    expect(
      commands.where(
        (command) =>
            command.$1 == 'input_terminal' &&
            object(command.$2?['input']).containsKey('text'),
      ),
      isEmpty,
    );
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    session.dispose();
  });

  testWidgets('keyboard resize coalesces and preserves the grid', (
    tester,
  ) async {
    addTearDown(tester.view.resetViewInsets);
    final connection = ConnectionFixture();
    final commands = <(String, Map<String, dynamic>?)>[];
    final host = HostConnection.test(
      id: 'node',
      label: 'Host',
      connection: connection,
      command: (kind, data) async {
        commands.add((kind, data));
        if (kind == 'claim_terminal') {
          connection.updates.emit(view(2));
          return {'data': info(2)};
        }
        return {'data': {}};
      },
    );
    final session = AppSession.test(hosts: [host]);
    connection.updates.emit(view(1));
    await mount(tester, session);
    final grid = find.byWidgetPredicate(
      (widget) =>
          widget is CustomPaint && widget.painter is TerminalGridPainter,
    );
    final original = tester.element(grid);
    await tester.tap(
      find.widgetWithText(FilledButton, tr('resourceTerminalControl')),
    );
    await tester.pumpAndSettle();
    await tester.pump(const Duration(milliseconds: 200));
    final initial = commands
        .singleWhere((command) => command.$1 == 'resize_terminal')
        .$2!['viewport'];
    commands.clear();

    Future<void> inset(double bottom) async {
      tester.view.viewInsets = FakeViewPadding(bottom: bottom);
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 16));
      expect(tester.element(grid), same(original));
      final painter =
          tester.widget<CustomPaint>(grid).painter! as TerminalGridPainter;
      expect(
        terminalLines(painter.screen).first['spans'][0]['text'],
        'Actual ',
      );
      expect(
        commands.where((command) => command.$1 == 'resize_terminal'),
        isEmpty,
      );
    }

    for (final height in [60.0, 130.0, 220.0, 300.0]) {
      await inset(height);
    }
    await tester.pump(const Duration(milliseconds: 200));
    final opened = commands
        .singleWhere((command) => command.$1 == 'resize_terminal')
        .$2!['viewport'];
    expect(opened['rows'], lessThan(initial['rows']));
    expect(opened['columns'], initial['columns']);
    commands.clear();

    await tester.tap(find.byTooltip(tr('terminalHideKeyboard')));
    await tester.pump();
    expect(
      tester.widget<EditableText>(find.byType(EditableText)).focusNode.hasFocus,
      isFalse,
    );
    for (final height in [220.0, 130.0, 60.0, 0.0]) {
      await inset(height);
    }
    await tester.pump(const Duration(milliseconds: 200));
    expect(
      commands
          .singleWhere((command) => command.$1 == 'resize_terminal')
          .$2!['viewport'],
      initial,
    );
    commands.clear();

    await tester.tap(find.byTooltip(tr('terminalKeyboard')));
    await tester.pump();
    expect(tester.testTextInput.isVisible, isTrue);
    await inset(150);
    await inset(0);
    await tester.pump(const Duration(milliseconds: 200));
    expect(
      commands.where((command) => command.$1 == 'resize_terminal'),
      isEmpty,
    );

    final shortcuts = find.descendant(
      of: find.byType(TerminalToolbar),
      matching: find.byType(SingleChildScrollView),
    );
    final writes = commands
        .where((command) => command.$1 == 'input_terminal')
        .length;
    await tester.drag(shortcuts, const Offset(-600, 0));
    await tester.pumpAndSettle();
    expect(
      commands.where((command) => command.$1 == 'input_terminal'),
      hasLength(writes),
    );
    final enter = find.byTooltip(tr('terminalEnter'));
    await tester.ensureVisible(enter);
    await tester.tap(enter);
    await tester.pump();
    expect(commands.last.$2!['input']['key']['event']['key'], 'enter');
    expect(
      tester.widget<EditableText>(find.byType(EditableText)).focusNode.hasFocus,
      isTrue,
    );

    commands.clear();
    await inset(150);
    connection.updates.emit(view(3));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    expect(
      commands.where((command) => command.$1 == 'resize_terminal'),
      isEmpty,
    );
    expect(
      find.byKey(const ValueKey('terminal-control-overlay')),
      findsOneWidget,
    );
    expect(connection.watched, ['terminal']);
    expect(tester.takeException(), isNull);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    session.dispose();
  });

  test('selection preserves cell widths and wrapped graphemes', () {
    final lines = <Map<String, dynamic>>[
      {
        'wrapped': true,
        'spans': [
          {'column': 0, 'columns': 2, 'text': 'ab'},
          {'column': 2, 'columns': 2, 'text': '中'},
        ],
      },
      {
        'wrapped': false,
        'spans': [
          {'column': 0, 'columns': 1, 'text': 'e\u0301'},
          {'column': 1, 'columns': 2, 'text': '😀'},
        ],
      },
      {
        'spans': [
          {'column': 0, 'columns': 3, 'text': 'end'},
        ],
      },
    ];
    expect(
      terminalSelection(lines, (row: 0, column: 1), (row: 2, column: 2)),
      'b中e\u0301😀\nend',
    );
    expect(
      terminalSelection(lines, (row: 1, column: 2), (row: 0, column: 3)),
      '中e\u0301😀',
    );
    expect(terminalViewport(const Size(415, 207), const Size(9, 21)), {
      'columns': 46,
      'rows': 9,
      'pixel_width': 414,
      'pixel_height': 189,
    });
    expect(terminalViewport(const Size(99999, 99999), const Size(9, 21)), {
      'columns': 500,
      'rows': 200,
      'pixel_width': 4500,
      'pixel_height': 4200,
    });
  });

  for (final binding in ['local', 'remote']) {
    testWidgets('$binding view ownership and single IME commit', (
      tester,
    ) async {
      final connection = ConnectionFixture();
      final commands = <(String, Map<String, dynamic>?)>[];
      final host = HostConnection.test(
        id: 'node',
        label: binding,
        connection: connection,
        command: (kind, data) async {
          commands.add((kind, data));
          if (kind == 'claim_terminal') {
            connection.updates.emit(view(2));
            return {'data': info(2)};
          }
          return {'data': {}};
        },
      );
      final session = AppSession.test(hosts: [host]);
      connection.updates.emit(view(1));
      await mount(tester, session);
      expect(connection.watched, ['terminal']);
      expect(commands.map((entry) => entry.$1), ['open_terminal']);
      expect(commands.single.$2!['terminal'], 'terminal');
      expect(commands.single.$2!['viewport'], isNotNull);
      commands.clear();
      expect(find.byType(EmptyState), findsOneWidget);
      expect(find.text(tr('resourceTerminalControlHint')), findsOneWidget);
      expect(find.byType(FilledButton), findsOneWidget);
      await tester.tap(
        find.widgetWithText(FilledButton, tr('resourceTerminalControl')),
      );
      await tester.pumpAndSettle();
      expect(commands.first.$1, 'claim_terminal');
      expect(
        find.byKey(const ValueKey('terminal-control-overlay')),
        findsNothing,
      );
      expect(commands.first.$2, {
        'terminal': 'terminal',
        'expected_revision': 1,
      });
      final input = find.byType(EditableText);
      expect(tester.widget<EditableText>(input).readOnly, isFalse);
      expect(
        tester.widget<EditableText>(input).keyboardType,
        TextInputType.text,
      );
      expect(tester.widget<EditableText>(input).obscureText, isFalse);
      expect(tester.widget<EditableText>(input).enableSuggestions, isTrue);
      expect(tester.widget<EditableText>(input).autocorrect, isFalse);
      final writesBefore = commands
          .where((entry) => entry.$1 == 'input_terminal')
          .length;
      tester.testTextInput.updateEditingValue(
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
      expectCompositionFits(tester, input);
      expect(tester.getSize(input).height, greaterThan(10));
      expect(
        commands.where((entry) => entry.$1 == 'input_terminal'),
        hasLength(writesBefore),
      );
      tester.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: '中文😀',
          selection: TextSelection.collapsed(offset: 4),
        ),
      );
      await tester.pump();
      expect(
        tester
            .widget<Opacity>(find.byKey(const ValueKey('terminal-ime')))
            .opacity,
        0,
      );
      final committed = commands
          .where(
            (entry) =>
                entry.$1 == 'input_terminal' &&
                entry.$2!['input'].containsKey('text'),
          )
          .toList();
      expect(committed, hasLength(1));
      expect(committed.single.$2, {
        'terminal': 'terminal',
        'revision': 2,
        'input': {
          'text': {'text': '中文😀'},
        },
      });
      await tester.tap(find.text('Ctrl'));
      await tester.pump();
      tester.testTextInput.updateEditingValue(
        const TextEditingValue(
          text: 'c',
          selection: TextSelection.collapsed(offset: 1),
        ),
      );
      await tester.pump();
      final chord = commands
          .where(
            (entry) =>
                entry.$1 == 'input_terminal' &&
                entry.$2!['input'].containsKey('key'),
          )
          .last
          .$2!;
      expect(chord['input']['key']['event']['key'], {
        'character': {'codepoint': 99},
      });
      expect(chord['input']['key']['event']['modifiers'], {
        'shift': false,
        'control': true,
        'alt': false,
        'super_key': false,
        'caps_lock': false,
        'num_lock': false,
      });
      final count = commands.length;
      connection.updates.emit(view(3));
      await tester.pumpAndSettle();
      expect(tester.widget<EditableText>(input).readOnly, isTrue);
      await tester.tap(find.text('Esc'));
      await tester.pump();
      expect(commands, hasLength(count));
      await tester.pumpWidget(const SizedBox());
      await tester.pump();
      expect(connection.updates.closed, isTrue);
      expect(connection.updates.released, isTrue);
      expect(commands.where((entry) => entry.$1 == 'close_terminal'), isEmpty);
      session.dispose();
    });
  }
}
