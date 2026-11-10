import 'dart:math' show max, min;

import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:url_launcher/url_launcher.dart';

import '../l10n/strings.dart';
import '../ui/theme.dart';
import '../ui/toast.dart';
import 'chart.dart';
import 'image_preview.dart';
import 'code.dart';
import 'math.dart';
import 'visuals.dart';

/// Shared message and file rendering leaves the original Markdown intact.
class MarkdownContent extends StatelessWidget {
  const MarkdownContent(
    this.data, {
    super.key,
    this.muted = false,
    this.selectable = true,
    this.imageBuilder,
    this.onOpenFile,
    this.onOpenLink,
  });

  final String data;
  final bool muted;

  /// Off where a long press opens message actions instead of selection.
  final bool selectable;
  final MarkdownImageBuilder? imageBuilder;
  final Future<void> Function(Uri uri)? onOpenFile;

  /// Opens a tapped link before the defaults and reports whether it did.
  final bool Function(String link)? onOpenLink;

  @override
  Widget build(BuildContext context) {
    final content = MarkdownBody(
      data: data,
      builders: {
        'pre': _CodeBuilder(),
        'sailry-math-inline': MathBuilder(),
        'sailry-math-block': MathBuilder(block: true),
      },
      blockSyntaxes: const [MathBlockSyntax()],
      inlineSyntaxes: [
        MathInlineSyntax(),
        ContentLinks(images: imageBuilder != null),
      ],
      imageBuilder: (uri, title, alt) => SizedBox(
        width: double.infinity,
        child: Align(
          alignment: Alignment.centerLeft,
          child: imageBuilder?.call(uri, title, alt) ?? uriImage(uri),
        ),
      ),
      selectable: selectable,
      fitContent: false,
      styleSheet: contentStyleSheet(Theme.of(context), muted: muted),
      onTapLink: (_, href, _) async {
        if (href != null && onOpenLink?.call(href) == true) return;
        final uri = Uri.tryParse(href ?? '');
        try {
          if (uri != null) {
            if ((uri.scheme.isEmpty || uri.scheme == 'file') &&
                onOpenFile != null) {
              await onOpenFile!(uri);
              return;
            }
            if (['http', 'https', 'mailto'].contains(uri.scheme) &&
                await launchUrl(uri)) {
              return;
            }
          }
        } catch (_) {
          // Keep unsupported links and failed opens visible to the user.
        }
        if (context.mounted) {
          showToast(context, context.tr('fileLinkUnavailable'));
        }
      },
    );
    // Tables and code keep horizontal scrolling; the library's scrollbar
    // draws over the rows, so it stays hidden.
    final scoped = ScrollbarTheme(
      data: const ScrollbarThemeData(
        thumbVisibility: WidgetStatePropertyAll(false),
        thickness: WidgetStatePropertyAll(0),
        thumbColor: WidgetStatePropertyAll(Colors.transparent),
      ),
      child: content,
    );
    return VisualScope.contains(context) ? scoped : VisualScope(child: scoped);
  }
}

MarkdownStyleSheet contentStyleSheet(ThemeData theme, {bool muted = false}) {
  final colors = theme.colorScheme;
  final body = TextStyle(
    fontSize: 14,
    height: 1.7,
    color: muted ? colors.onSurfaceVariant : colors.onSurface,
  );
  return MarkdownStyleSheet.fromTheme(theme).copyWith(
    p: body,
    a: TextStyle(color: SailryTheme.link(theme.brightness)),
    tableHead: body.copyWith(fontWeight: FontWeight.w600),
    tableBody: body,
    tableHeadAlign: TextAlign.left,
    // Fixed columns activate the library's per-table horizontal scroller.
    // Long cells wrap within a readable width instead of squeezing to fit.
    tableColumnWidth: const FixedColumnWidth(220),
    tableScrollbarThumbVisibility: false,
    tableCellsPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
    tableBorder: TableBorder(
      horizontalInside: BorderSide(color: colors.outlineVariant),
    ),
    code: TextStyle(
      fontSize: body.fontSize,
      height: body.height,
      fontFamily: 'monospace',
      backgroundColor: colors.surfaceContainerHigh,
    ),
    // Every fenced block supplies its own CodeBlock, chart or diagram.
    codeblockDecoration: const BoxDecoration(),
    blockquoteDecoration: BoxDecoration(
      border: Border(left: BorderSide(color: colors.outlineVariant, width: 3)),
    ),
  );
}

/// Reuse Markdown's link parser so references, escapes, and code keep their semantics.
class ContentLinks extends md.LinkSyntax {
  ContentLinks({required this.images});
  final bool images;

  /// The last text link closed and where its source ends. It takes its
  /// trailing gap once a link directly after it shows which character follows.
  ({md.InlineParser parser, int end, md.Element link})? _previous;

  @override
  Iterable<md.Node>? close(
    md.InlineParser parser,
    covariant md.SimpleDelimiter opener,
    md.Delimiter? closer, {
    String? tag,
    required List<md.Node> Function() getChildren,
  }) {
    final nodes = super.close(
      parser,
      opener,
      closer,
      tag: tag,
      getChildren: getChildren,
    );
    final link = nodes?.singleOrNull;
    if (link is! md.Element ||
        link.tag != 'a' ||
        link.textContent.isEmpty ||
        link.children!.any((node) => node is md.Element && node.tag == 'img')) {
      return nodes;
    }
    // Neighbours are read from the Markdown source, so emphasis or code
    // markup touching a link counts as punctuation.
    final start = opener.endPos - 1;
    final end = parser.pos + 1;
    final text = link.textContent;
    var before = _charBefore(parser.source, start);
    final previous = _previous;
    if (previous != null &&
        identical(previous.parser, parser) &&
        previous.end == start) {
      final last = previous.link.textContent;
      before = _charBefore(last, last.length);
      if (needsLinkGap(_charAfter(text, 0))) {
        previous.link.children!.add(md.Text(_linkGap));
      }
    }
    if (needsLinkGap(before)) link.children!.insert(0, md.Text(_linkGap));
    if (needsLinkGap(_charAfter(parser.source, end))) {
      link.children!.add(md.Text(_linkGap));
    }
    _previous = (parser: parser, end: end, link: link);
    return nodes;
  }

  @override
  md.Node createNode(
    String destination,
    String? title, {
    required List<md.Node> Function() getChildren,
  }) {
    final link =
        super.createNode(
              destination,
              title,
              getChildren: () => getChildren().map(literalLinkMath).toList(),
            )
            as md.Element;
    final uri = Uri.tryParse(link.attributes['href']!);
    if (uri == null ||
        (uri.scheme.isNotEmpty && uri.scheme != 'file') ||
        uri.hasAuthority && uri.host.isNotEmpty ||
        link.children!.any((node) => node is md.Element && node.tag == 'img')) {
      return link;
    }
    if (images &&
        RegExp(
          r'\.(png|jpe?g|gif|webp|bmp)$',
          caseSensitive: false,
        ).hasMatch(uri.path)) {
      return md.Element.empty('img')
        ..attributes.addAll({
          'src': link.attributes['href']!,
          'alt': link.textContent,
          'title': ?link.attributes['title'],
        });
    }
    // Other file links stay inline like ordinary links; taps open them.
    return link;
  }
}

/// Room between a link and a word beside it. Selectable text copies a
/// WidgetSpan as U+FFFC, so the gap is a real narrow no-break space that
/// copied text includes. It also keeps the link with its neighbour on wrap.
const _linkGap = '\u202f';

/// Whether a link needs room from [neighbour], the character beside it, or
/// null at a paragraph edge. An edge and whitespace need none, and punctuation
/// binds to its neighbour or carries its own full-width spacing.
bool needsLinkGap(String? neighbour) {
  if (neighbour == null || neighbour.trim().isEmpty) return false;
  final char = neighbour.runes.first;
  return !_punctuation.any((range) => char >= range.$1 && char <= range.$2);
}

const _punctuation = [
  // ASCII punctuation.
  (0x21, 0x2f), (0x3a, 0x40), (0x5b, 0x60), (0x7b, 0x7e),
  // Latin-1 marks: inverted exclamation, section, guillemets, pilcrow,
  // middle dot and inverted question.
  (0xa1, 0xa1), (0xa7, 0xa7), (0xab, 0xab), (0xb6, 0xb7), (0xbb, 0xbb),
  (0xbf, 0xbf),
  // General Punctuation: dashes, quotes, ellipses and invisible joiners.
  (0x2000, 0x206f),
  // CJK Symbols and Punctuation, then vertical and compatibility forms.
  (0x3000, 0x303f), (0xfe10, 0xfe1f), (0xfe30, 0xfe6f),
  // Full-width ASCII punctuation and half-width CJK marks.
  (0xff01, 0xff0f), (0xff1a, 0xff20), (0xff3b, 0xff40), (0xff5b, 0xff65),
];

// Two code units always cover the surrogate pair of one character.
String? _charBefore(String text, int index) => index > 0
    ? String.fromCharCode(text.substring(max(0, index - 2), index).runes.last)
    : null;

String? _charAfter(String text, int index) => index < text.length
    ? String.fromCharCode(
        text.substring(index, min(text.length, index + 2)).runes.first,
      )
    : null;

class _CodeBuilder extends MarkdownElementBuilder {
  @override
  bool isBlockElement() => true;
  @override
  Widget? visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final code = element.children?.whereType<md.Element>().firstOrNull;
    final tag = code?.attributes['class'];
    final language = tag?.startsWith('language-') == true
        ? tag!.substring(9)
        : null;
    return switch (language) {
      'chart' => DecoratedBox(
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHigh,
          borderRadius: BorderRadius.circular(8),
        ),
        child: SailryChart(source: element.textContent),
      ),
      'mermaid' => MarkdownVisual(
        kind: VisualKind.mermaid,
        source: element.textContent,
      ),
      _ => CodeBlock(element.textContent, language: language),
    };
  }
}
