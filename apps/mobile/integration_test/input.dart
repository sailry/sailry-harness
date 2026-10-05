import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// The debug-only test input client is not available in profile builds.
Future<void> edit(WidgetTester tester, Finder finder, String value) async {
  final editor = tester.state<EditableTextState>(
    find.descendant(
      of: finder,
      matching: find.byType(EditableText),
      matchRoot: true,
    ),
  );
  editor.requestKeyboard();
  await tester.pump();
  editor.updateEditingValue(
    TextEditingValue(
      text: value,
      selection: TextSelection.collapsed(offset: value.length),
    ),
  );
  await tester.pump();
}

void expectCompositionFits(WidgetTester tester, Finder finder) {
  final state = tester.state<EditableTextState>(finder);
  final render = state.renderEditable;
  final range = state.widget.controller.value.composing;
  final bounds = render.getRectForComposingRange(range);
  expect(bounds, isNotNull);
  // Check rendered glyphs, including the editor's horizontal scroll offset.
  expect(bounds!.width, greaterThan(0));
  expect(bounds.left, greaterThanOrEqualTo(0));
  expect(bounds.right, lessThanOrEqualTo(render.size.width));
  final caret = render.getLocalRectForCaret(TextPosition(offset: range.end));
  expect(caret.left, greaterThanOrEqualTo(0));
  expect(caret.right, lessThanOrEqualTo(render.size.width));
}
