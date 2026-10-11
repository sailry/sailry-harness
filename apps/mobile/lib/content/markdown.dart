import 'package:flutter/material.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:url_launcher/url_launcher.dart';

import '../l10n/strings.dart';
import '../ui/icons.dart';
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
    this.imageBuilder,
    this.onOpenFile,
  });

  final String data;
  final bool muted;
  final MarkdownImageBuilder? imageBuilder;
  final Future<void> Function(Uri uri)? onOpenFile;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final colors = theme.colorScheme;
    final body = TextStyle(
      fontSize: 14,
      height: 1.7,
      color: muted ? colors.onSurfaceVariant : colors.onSurface,
    );
    final content = MarkdownBody(
      data: data,
      builders: {
        'pre': _CodeBuilder(),
        'sailry-math-inline': MathBuilder(),
        'sailry-math-block': MathBuilder(block: true),
        if (onOpenFile != null) 'sailry-file': _FileBuilder(onOpenFile!),
      },
      blockSyntaxes: const [MathBlockSyntax()],
      inlineSyntaxes: [
        MathInlineSyntax(),
        _ContentLinks(images: imageBuilder != null, files: onOpenFile != null),
      ],
      imageBuilder: (uri, title, alt) => SizedBox(
        width: double.infinity,
        child: Align(
          alignment: Alignment.centerLeft,
          child: imageBuilder?.call(uri, title, alt) ?? uriImage(uri),
        ),
      ),
      selectable: true,
      fitContent: false,
      styleSheet: MarkdownStyleSheet.fromTheme(theme).copyWith(
        p: body,
        tableHead: body.copyWith(fontWeight: FontWeight.w600),
        tableBody: body,
        tableHeadAlign: TextAlign.left,
        // Fixed columns activate the library's per-table horizontal scroller.
        // Long cells wrap within a readable width instead of squeezing to fit.
        tableColumnWidth: const FixedColumnWidth(220),
        tableScrollbarThumbVisibility: true,
        tableCellsPadding: const EdgeInsets.symmetric(
          horizontal: 12,
          vertical: 10,
        ),
        tableBorder: TableBorder(
          horizontalInside: BorderSide(color: colors.outlineVariant),
        ),
        code: body.copyWith(
          fontFamily: 'monospace',
          backgroundColor: colors.surfaceContainerHigh,
        ),
        // Every fenced block supplies its own CodeBlock, chart or diagram.
        codeblockDecoration: const BoxDecoration(),
        blockquoteDecoration: BoxDecoration(
          border: Border(
            left: BorderSide(color: colors.outlineVariant, width: 3),
          ),
        ),
      ),
      onTapLink: (_, href, _) async {
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
    return VisualScope.contains(context)
        ? content
        : VisualScope(child: content);
  }
}

/// Reuse Markdown's link parser so references, escapes, and code keep their semantics.
class _ContentLinks extends md.LinkSyntax {
  _ContentLinks({required this.images, required this.files});
  final bool images;
  final bool files;
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
    if (files &&
        RegExp(
          r'\.(pdf|docx?|xlsx?|pptx?|odt|ods|odp|html?|md|markdown|txt|csv|json|ya?ml|toml|xml|rs|dart|py|js|ts|tsx|jsx|css|sh|sql|go|java|c|cpp|h|hpp)$',
          caseSensitive: false,
        ).hasMatch(uri.path)) {
      return md.Element.text('sailry-file', link.textContent)
        ..attributes['href'] = link.attributes['href']!;
    }
    return link;
  }
}

class _FileBuilder extends MarkdownElementBuilder {
  _FileBuilder(this.open);
  final Future<void> Function(Uri) open;

  @override
  Widget visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final uri = Uri.parse(element.attributes['href']!);
    final name = uri.path.split('/').last;
    return SizedBox(
      width: double.infinity,
      child: Card.outlined(
        margin: const EdgeInsets.symmetric(vertical: 4),
        child: ListTile(
          leading: const AppIcon('file'),
          title: Text(element.textContent.isEmpty ? name : element.textContent),
          subtitle: element.textContent == name ? null : Text(name),
          onTap: () async {
            try {
              await open(uri);
            } catch (_) {
              if (context.mounted) {
                showToast(context, context.tr('fileLinkUnavailable'));
              }
            }
          },
        ),
      ),
    );
  }
}

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
