import 'package:flutter/material.dart';
import 'package:html/dom.dart' as dom;
import 'package:html/parser.dart' show parseFragment;

import '../../../content/links.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/theme.dart';
import '../../../ui/toast.dart';
import 'links.dart';

/// Provider metadata accompanies the unchanged answer. HTML is parsed into
/// native text and links; provider scripts, styles and embedded pages never run.
class EntrySources extends StatelessWidget {
  const EntrySources({super.key, required this.entry, this.links});

  final Map<String, dynamic> entry;
  final ConversationLinks? links;

  @override
  Widget build(BuildContext context) {
    final citations = objects(entry['citations']);
    final suggestions = _suggestions(text(entry['search_suggestions']));
    if (citations.isEmpty && suggestions.isEmpty) {
      return const SizedBox.shrink();
    }
    final theme = Theme.of(context);
    Widget heading(String key) => Padding(
      padding: const EdgeInsets.only(top: 8, bottom: 2),
      child: Text(
        context.tr(key),
        style: theme.textTheme.labelMedium?.copyWith(
          color: theme.colorScheme.onSurfaceVariant,
        ),
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (citations.isNotEmpty) heading('conversationSources'),
        for (var index = 0; index < citations.length; index++)
          _SourceLink(
            label: text(citations[index]['title']).trim().isNotEmpty
                ? text(citations[index]['title'])
                : text(citations[index]['uri']),
            uri: parseCitationLink(text(citations[index]['uri'])),
            number: index + 1,
            links: links,
          ),
        if (suggestions.isNotEmpty) heading('conversationSearchSuggestions'),
        for (final suggestion in suggestions)
          _SourceLink(
            label: suggestion.label,
            uri: suggestion.uri,
            links: links,
          ),
      ],
    );
  }
}

class _SourceLink extends StatelessWidget {
  const _SourceLink({
    required this.label,
    required this.uri,
    this.number,
    this.links,
  });

  final String label;
  final Uri? uri;
  final int? number;
  final ConversationLinks? links;

  @override
  Widget build(BuildContext context) => Semantics(
    link: uri != null,
    child: TextButton(
      onPressed: uri == null ? null : () => _open(context, uri!),
      style:
          TextButton.styleFrom(
            alignment: Alignment.centerLeft,
            padding: const EdgeInsets.symmetric(vertical: 6),
            minimumSize: const Size(0, 36),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ).copyWith(
            foregroundColor: ButtonStyleButton.defaultColor(
              SailryTheme.link(Theme.of(context).brightness),
              Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (number != null) ...[Text('[$number]'), const SizedBox(width: 8)],
          Expanded(child: Text(label)),
        ],
      ),
    ),
  );

  Future<void> _open(BuildContext context, Uri uri) async {
    if (links != null) {
      await links!.openUri(context, uri);
      return;
    }
    try {
      if (await launchExternalContentLink(uri)) return;
    } catch (_) {
      // The platform may reject an otherwise valid web link.
    }
    if (context.mounted) showToast(context, context.tr('conversationFailed'));
  }
}

typedef _Suggestion = ({String label, Uri? uri});

List<_Suggestion> _suggestions(String html) {
  if (html.isEmpty) return const [];
  final fragment = parseFragment(html);
  for (final element in fragment.querySelectorAll(
    'script, style, svg, template, iframe, object, embed, link, meta, [hidden]',
  )) {
    element.remove();
  }
  final result = <_Suggestion>[];
  final pending = StringBuffer();
  String normalize(String value) =>
      value.replaceAll(RegExp(r'\s+'), ' ').trim();
  void flush() {
    final label = normalize(pending.toString());
    pending.clear();
    if (label.isNotEmpty) result.add((label: label, uri: null));
  }

  void visit(dom.Node node) {
    if (node is dom.Text) {
      pending.write(node.data);
      return;
    }
    if (node is dom.Element && node.localName == 'a') {
      flush();
      final href = node.attributes['href'] ?? '';
      final label = normalize(node.text);
      if (label.isNotEmpty || href.isNotEmpty) {
        result.add((
          label: label.isEmpty ? href : label,
          uri: parseCitationLink(href),
        ));
      }
      return;
    }
    final block =
        node is dom.Element &&
        const {
          'div',
          'p',
          'li',
          'br',
          'section',
          'header',
        }.contains(node.localName);
    if (block) flush();
    for (final child in node.nodes) {
      visit(child);
    }
    if (block) flush();
  }

  visit(fragment);
  flush();
  return result;
}
