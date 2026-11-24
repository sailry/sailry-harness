import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:sailry_mobile/content/code.dart';
import 'package:sailry_mobile/content/markdown.dart';
import 'package:sailry_mobile/content/visuals.dart';

Future<void> pumpContent(WidgetTester tester, Widget child) =>
    tester.pumpWidget(
      MaterialApp(
        home: Scaffold(body: SingleChildScrollView(child: child)),
      ),
    );

// A standalone block is a SelectableText; inside a message's selection
// area the same code renders as plain text joining that selection.
final codeText = find
    .descendant(
      of: find.byType(CodeText),
      matching: find.byWidgetPredicate(
        (widget) => widget is SelectableText || widget is Text,
      ),
    )
    .first;

String displayedCode(WidgetTester tester) => switch (tester.widget(codeText)) {
  SelectableText(:final textSpan) => textSpan!.toPlainText(),
  Text(:final textSpan) => textSpan!.toPlainText(),
  _ => throw StateError('code text is missing'),
};

void main() {
  const css = 'body { color: red; }';

  for (final width in [320.0, 768.0]) {
    testWidgets('single-line CSS fence has one text line at width $width', (
      tester,
    ) async {
      tester.view.physicalSize = Size(width, 800);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.reset);

      await pumpContent(tester, const CodeBlock(css, language: 'css'));
      final singleLineHeight = tester.getSize(codeText).height;
      final blockHeight = tester.getSize(find.byType(CodeBlock)).height;

      await pumpContent(tester, const MarkdownContent('```css\n$css\n```'));
      expect(displayedCode(tester), css);
      expect(tester.getSize(codeText).height, singleLineHeight);
      expect(tester.getSize(find.byType(CodeBlock)).height, blockHeight);
      expect(tester.takeException(), isNull);
    });
  }

  for (final fixture in [
    (
      name: 'CSS',
      markdown: '```css\n$css\n```',
      display: css,
      source: '$css\n',
    ),
    (name: 'plain', markdown: '```\n$css\n```', display: css, source: '$css\n'),
    (
      name: 'unknown language',
      markdown: '```unknown-language\n$css\n```',
      display: css,
      source: '$css\n',
    ),
    (
      name: 'intentional whitespace',
      markdown: '```css\n\n$css  \n\n\n```',
      display: '\n$css  \n\n',
      source: '\n$css  \n\n\n',
    ),
    (
      name: 'multiple lines',
      markdown: '```css\nbody {\n  color: red;\n}\n```',
      display: 'body {\n  color: red;\n}',
      source: 'body {\n  color: red;\n}\n',
    ),
    (name: 'empty', markdown: '```css\n```', display: '', source: ''),
    (
      name: 'unfinished fence',
      markdown: '```css\n$css',
      display: css,
      source: '$css\n',
    ),
    (
      name: 'CRLF input',
      markdown: '```css\r\n$css\r\n```',
      display: css,
      source: '$css\n',
    ),
  ]) {
    testWidgets('${fixture.name} keeps source separate from display', (
      tester,
    ) async {
      await pumpContent(tester, MarkdownContent(fixture.markdown));
      expect(displayedCode(tester), fixture.display);
      expect(
        tester.widget<CodeBlock>(find.byType(CodeBlock)).text,
        fixture.source,
      );
      expect(
        tester.widget<CopyTextButton>(find.byType(CopyTextButton)).text,
        fixture.source,
      );
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('copy button retains the parsed source including blank lines', (
    tester,
  ) async {
    String? copied;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copied = (call.arguments as Map)['text'] as String;
        }
        return null;
      },
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    await pumpContent(tester, const MarkdownContent('```css\n$css  \n\n```'));
    await tester.tap(find.byType(CopyTextButton));
    await tester.pumpAndSettle();
    expect(copied, '$css  \n\n');
    await tester.pump(const Duration(seconds: 3));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('ordinary CodeBlock preserves its trailing newlines', (
    tester,
  ) async {
    const source = '$css\n\n';
    await pumpContent(tester, const CodeBlock(source, language: 'css'));
    expect(displayedCode(tester), source);
    expect(
      tester.widget<CopyTextButton>(find.byType(CopyTextButton)).text,
      source,
    );
  });

  for (final fails in [false, true]) {
    testWidgets('Mermaid source stays intact when rendering fails=$fails', (
      tester,
    ) async {
      const source = 'graph TD\n  A --> B\n\n';
      final result = Completer<VisualSvg>();
      final requests = <VisualRequest>[];
      await pumpContent(
        tester,
        VisualScope(
          render: (request) {
            requests.add(request);
            return result.future;
          },
          child: const MarkdownContent('```mermaid\n$source```'),
        ),
      );
      final visual = tester.widget<MarkdownVisual>(find.byType(MarkdownVisual));
      expect(visual.kind, VisualKind.mermaid);
      expect(visual.source, source);
      expect(requests.single.source, source);
      expect(displayedCode(tester), source);

      if (fails) {
        result.completeError(StateError('Fixture render failure'));
      } else {
        result.complete(
          const VisualSvg(
            source:
                '<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><path d="M0 0L20 10"/></svg>',
            width: 20,
            height: 10,
          ),
        );
      }
      await tester.pumpAndSettle();
      if (fails) {
        expect(displayedCode(tester), source);
      } else {
        expect(find.byType(CodeBlock), findsNothing);
      }
      expect(
        tester.widget<CopyTextButton>(find.byType(CopyTextButton)).text,
        source,
      );
      expect(tester.takeException(), isNull);
      await tester.pumpWidget(const SizedBox());
    });
  }
}
