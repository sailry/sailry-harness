import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/demo/connection.dart';
import 'package:sailry_mobile/features/terminal/grid.dart';
import 'package:sailry_mobile/features/terminal/terminal_page.dart';
import 'package:sailry_mobile/l10n/strings.dart';
import 'package:sailry_mobile/runtime/session.dart';
import 'package:sailry_mobile/runtime/json.dart';

Future<void> command(
  DemoConnection connection,
  String kind,
  Map<String, dynamic> data,
) async {
  final request = await connection.prepare(
    command: jsonEncode({'kind': kind, 'data': data}),
  );
  final result = object(jsonDecode(await connection.execute(request: request)));
  expect(result, contains('Ok'));
}

Map<String, dynamic> inputText(String text) => {
  'text': {'text': text},
};

Map<String, dynamic> inputKey(String key, {String action = 'press'}) => {
  'key': {
    'event': {'key': key, 'action': action},
  },
};

String terminalText(Map<String, dynamic> view) => utf8.decode(
  (object(object(object(view['snapshot'])['screen'])['vt'])['bytes'] as List)
      .cast<int>(),
);

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  test(
    'local shell reads the same workspace and Git state as resource pages',
    () async {
      final connection = DemoConnection(translate: tr);
      final updates = await connection.watchTerminal(terminal: 'demo-terminal');
      await updates.next();
      await command(connection, 'write_file', {
        'path': 'src/styles/login.css',
        'text': '.login-form { gap: 24px; }',
        'expected_revision': '1',
      });
      await command(connection, 'update_git_index', {'operation': 'stage'});
      await command(connection, 'input_terminal', {
        'inputs': [
          inputText(
            'pwd\nls\ncd src\npwd\ncat styles/login.css\ngit status --short\n',
          ),
        ],
      });
      final output = terminalText(object(jsonDecode(await updates.next())));
      expect(output, contains('/Users/alex/Projects/lumen-web'));
      expect(output, contains('/Users/alex/Projects/lumen-web/src'));
      expect(output, contains('README.md'));
      expect(output, contains('.login-form { gap: 24px; }'));
      expect(output, contains('M  src/styles/login.css'));
      expect(output, isNot(contains('demoTerminalReply')));
      await updates.close();
      updates.dispose();
      await connection.close();
    },
  );

  test(
    'terminal editing, history and CRLF paste execute complete lines once',
    () async {
      final connection = DemoConnection(translate: tr);
      final updates = await connection.watchTerminal(terminal: 'demo-terminal');
      await updates.next();
      await command(connection, 'input_terminal', {
        'inputs': [
          inputText('echo hx'),
          inputKey('backspace'),
          inputText('i'),
          inputKey('enter'),
          inputKey('enter', action: 'release'),
          inputKey('arrow_up'),
          inputKey('enter'),
          {
            'paste': {'text': 'echo "pasted"\r\n'},
          },
        ],
      });
      final output = terminalText(object(jsonDecode(await updates.next())));
      expect(RegExp(r'\r\nhi\r\n').allMatches(output), hasLength(2));
      expect(RegExp(r'\r\npasted\r\n').allMatches(output), hasLength(1));
      await command(connection, 'input_terminal', {
        'inputs': [inputText('clear\n')],
      });
      final cleared = terminalText(object(jsonDecode(await updates.next())));
      expect(cleared, endsWith('alex@MacBook-Pro lumen-web % '));
      expect(cleared, isNot(contains('pasted')));
      await command(connection, 'resize_terminal', {
        'viewport': {'columns': 34, 'rows': 12},
      });
      final resized = object(jsonDecode(await updates.next()));
      expect(
        object(object(object(resized['snapshot'])['screen'])['vt'])['viewport'],
        {'columns': 34, 'rows': 12},
      );
      await updates.close();
      updates.dispose();
      await connection.close();
    },
  );

  testWidgets('local terminal keeps input after appearance and resize', (
    tester,
  ) async {
    final session = AppSession.test(
      initialize: () async => fail('Terminal opened native transport'),
    );
    final host = await session.pairPin('888888');
    await tester.pumpWidget(
      MaterialApp(
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: SessionScope(
          session: session,
          child: TerminalPage(
            hostId: host.id,
            worktreeId: 'demo-lumen-main',
            title: 'zsh',
          ),
        ),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pump(const Duration(seconds: 1));
    await tester.pump();
    final input = find.byType(EditableText);
    expect(input, findsOneWidget);
    expect(tester.widget<EditableText>(input).readOnly, isFalse);
    expect(find.text(tr('resourceTerminalControlHint')), findsNothing);
    expect(tester.takeException(), isNull);

    await tester.enterText(input, 'echo ready');
    await tester.testTextInput.receiveAction(TextInputAction.send);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 200));
    final painters = tester
        .widgetList<CustomPaint>(find.byType(CustomPaint))
        .map((widget) => widget.painter)
        .whereType<TerminalGridPainter>();
    final screen = painters
        .map((painter) => terminalLines(painter.screen))
        .expand((v) => v);
    final visible = screen
        .map(
          (row) => terminalSelection([row], (row: 0, column: 0), (
            row: 0,
            column: 499,
          )),
        )
        .map((line) => line.trim())
        .toList();
    expect(visible.join('\n'), contains('echo ready'));
    expect(visible, contains('ready'));
    expect(tester.widget<EditableText>(input).readOnly, isFalse);
    expect(session.controller, isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    await session.close();
    session.dispose();
  });
}
