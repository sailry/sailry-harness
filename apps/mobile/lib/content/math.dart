import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;

import 'visuals.dart';

/// Extend the Markdown parser, so formulas never rewrite fenced or inline code.
class MathInlineSyntax extends md.InlineSyntax {
  MathInlineSyntax() : super(r'\$\$?|\\[\(\[]');

  @override
  bool tryMatch(md.InlineParser parser, [int? startMatchPos]) {
    final start = startMatchPos ?? parser.pos;
    final match = pattern.matchAsPrefix(parser.source, start);
    if (match == null) return false;
    final opening = match[0]!;
    if (opening.startsWith('\\')) {
      // A link is parsed only after its label. Keep its complete range protected.
      for (var index = 0; index < start; index++) {
        if (parser.source[index] == '\\') {
          index++;
          continue;
        }
        final end = _linkEnd(parser.source, index, parser.document);
        if (end != null && end > start) return false;
      }
    }
    final closing = switch (opening) {
      r'\(' => r'\)',
      r'\[' => r'\]',
      _ => opening,
    };
    final from = start + opening.length;
    if (opening == r'$' &&
        (from == parser.source.length || _space(parser.source[from]))) {
      return false;
    }
    var braces = 0;
    var at = from;
    while (at < parser.source.length) {
      final code = md.CodeSyntax().pattern.matchAsPrefix(parser.source, at);
      if (code != null) {
        at = code.end;
        continue;
      }
      final link = _linkEnd(parser.source, at, parser.document);
      if (link != null) {
        at = link;
        continue;
      }
      if (parser.source.startsWith(closing, at) && braces == 0) {
        final body = parser.source.substring(from, at);
        if (body.trim().isEmpty ||
            (opening == r'$' && _space(parser.source[at - 1]))) {
          return false;
        }
        final end = at + closing.length;
        parser.writeText();
        parser.addNode(
          md.Element.text('sailry-math-inline', body)
            ..attributes['display'] = '${opening == r'$$' || opening == r'\['}'
            ..attributes['original'] = parser.source.substring(start, end),
        );
        parser.consume(end - start);
        return true;
      }
      if (parser.source[at] == '\\') {
        at += 2;
        continue;
      }
      if (opening.startsWith(r'$')) {
        if (parser.source[at] == '{') braces++;
        if (parser.source[at] == '}') {
          if (braces == 0) return false;
          braces--;
        }
        if (parser.source[at] == r'$' && braces == 0) return false;
      }
      at++;
    }
    return false;
  }

  @override
  bool onMatch(md.InlineParser parser, Match match) => false;
}

bool _space(String value) => RegExp(r'\s').hasMatch(value);

// Skip a Markdown link's destination and label while finding a LaTeX closer.
int? _linkEnd(String source, int at, md.Document document) {
  if (source[at] != '[') return null;
  var depth = 1;
  var end = at + 1;
  while (end < source.length && depth > 0) {
    if (source[end] == '\\') {
      end += 2;
      continue;
    }
    if (source[end] == '[') depth++;
    if (source[end] == ']') depth--;
    end++;
  }
  if (depth != 0) return null;
  final label = source.substring(at + 1, end - 1);
  if (end == source.length || source[end] != '(') {
    var reference = label;
    var referenceEnd = end;
    if (end < source.length && source[end] == '[') {
      final closer = source.indexOf(']', end + 1);
      if (closer < 0) return null;
      final explicit = source.substring(end + 1, closer);
      if (explicit.isNotEmpty) reference = explicit;
      referenceEnd = closer + 1;
    }
    final normalized = reference
        .trim()
        .replaceAll(RegExp(r'\s+'), ' ')
        .toLowerCase();
    return document.linkReferences.containsKey(normalized)
        ? referenceEnd
        : null;
  }
  depth = 1;
  end++;
  while (end < source.length && depth > 0) {
    if (source[end] == '\\') {
      end += 2;
      continue;
    }
    if (source[end] == '(') depth++;
    if (source[end] == ')') depth--;
    end++;
  }
  return depth == 0 ? end : null;
}

class MathBlockSyntax extends md.BlockSyntax {
  const MathBlockSyntax();

  @override
  RegExp get pattern => RegExp(r'^ {0,3}(\$\$|\\\[)');

  (int, String, String)? _formula(md.BlockParser parser) {
    final match = pattern.firstMatch(parser.current.content);
    if (match == null) return null;
    final opening = match[1]!;
    final closing = opening == r'\[' ? r'\]' : r'$$';
    final lines = <String>[];
    for (var index = 0; ; index++) {
      final line = parser.peek(index)?.content;
      if (line == null || (index > 0 && line.trim().isEmpty)) return null;
      final body = index == 0 ? line.substring(match.end) : line;
      lines.add(body);
      if (!body.contains(closing)) continue;
      final inline = md.InlineParser(
        '$opening${lines.join('\n')}',
        parser.document,
      );
      if (!MathInlineSyntax().tryMatch(inline)) continue;
      final nodes = inline.parse();
      final formula = nodes.first as md.Element;
      if (nodes.skip(1).any((node) => node.textContent.trim().isNotEmpty)) {
        return null;
      }
      return (
        index + 1,
        formula.textContent.trim(),
        formula.attributes['original']!,
      );
    }
  }

  @override
  bool canParse(md.BlockParser parser) => _formula(parser) != null;

  @override
  md.Node parse(md.BlockParser parser) {
    final (count, source, original) = _formula(parser)!;
    for (var index = 0; index < count; index++) {
      parser.advance();
    }
    return md.Element.text('sailry-math-block', source)
      ..attributes['original'] = original;
  }
}

class MathBuilder extends MarkdownElementBuilder {
  MathBuilder({this.block = false});
  final bool block;

  @override
  bool isBlockElement() => block;

  @override
  Widget visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final display = block || element.attributes['display'] == 'true';
    final visual = MarkdownVisual(
      kind: display ? VisualKind.displayMath : VisualKind.inlineMath,
      source: element.textContent,
      original: element.attributes['original'] ?? element.textContent,
      style: parentStyle,
    );
    return display
        ? visual
        : SelectableText.rich(
            TextSpan(
              style: parentStyle,
              children: [
                WidgetSpan(
                  alignment: PlaceholderAlignment.baseline,
                  baseline: TextBaseline.alphabetic,
                  child: visual,
                ),
              ],
            ),
          );
  }
}

/// Standard Markdown escaping still applies to LaTeX-shaped link labels.
md.Node literalLinkMath(md.Node node) {
  if (node is! md.Element) return node;
  if (node.tag == 'sailry-math-inline' &&
      node.attributes['original']?.startsWith('\\') == true) {
    return md.Text(
      node.attributes['original']!.replaceAllMapped(
        RegExp(r'\\([\(\)\[\]])'),
        (match) => match[1]!,
      ),
    );
  }
  final children = node.children;
  if (children != null) {
    final restored = children.map(literalLinkMath).toList();
    children
      ..clear()
      ..addAll(restored);
  }
  return node;
}
