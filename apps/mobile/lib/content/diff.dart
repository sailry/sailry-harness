import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';
import '../ui/kit.dart';
import 'code.dart';

class DiffLine {
  const DiffLine(this.text, this.before, this.after, this.kind);
  final String text;
  final int? before;
  final int? after;
  final String kind;
}

List<DiffLine> parseLines(String patch, {bool added = false}) {
  if (added) {
    final lines = patch.split('\n');
    if (lines.last.isEmpty) lines.removeLast();
    return [
      for (final (index, line) in lines.indexed)
        DiffLine(line, null, index + 1, 'added'),
    ];
  }
  var before = 0;
  var after = 0;
  var hunk = false;
  final result = <DiffLine>[];
  for (final line in patch.split('\n')) {
    final header = RegExp(
      r'^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@',
    ).firstMatch(line);
    if (header != null) {
      before = int.parse(header[1]!);
      after = int.parse(header[2]!);
      hunk = true;
      result.add(DiffLine(line, null, null, 'header'));
    } else if (hunk && line.startsWith('+')) {
      result.add(DiffLine(line, null, after++, 'added'));
    } else if (hunk && line.startsWith('-')) {
      result.add(DiffLine(line, before++, null, 'removed'));
    } else if (hunk && line.startsWith(' ')) {
      result.add(DiffLine(line, before++, after++, 'context'));
    } else {
      if (line.startsWith('diff --git')) hunk = false;
      result.add(DiffLine(line, null, null, 'header'));
    }
  }
  return result;
}

class DiffView extends StatelessWidget {
  const DiffView({super.key, required this.diff});
  final Map<String, dynamic> diff;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final patch = text(diff['text']);
    final path = text(diff['path']);
    return Surface(
      padding: EdgeInsets.zero,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.only(left: 12),
            child: Row(
              children: [
                Expanded(child: Text(path)),
                CopyTextButton(patch),
              ],
            ),
          ),
          const Divider(height: 1),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 400),
            child: SingleChildScrollView(
              child: SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    for (final line in parseLines(
                      patch,
                      added: diff['format'] == 'added',
                    ))
                      ColoredBox(
                        color:
                            (line.kind == 'added'
                                    ? colors.tertiary
                                    : line.kind == 'removed'
                                    ? colors.error
                                    : colors.surface)
                                .withValues(alpha: .10),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 8),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                '${line.before ?? ''}'.padLeft(5),
                                style: TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 13,
                                  height: 1.6,
                                  color: colors.onSurfaceVariant,
                                ),
                              ),
                              Text(
                                '${line.after ?? ''}'.padLeft(5),
                                style: TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 13,
                                  height: 1.6,
                                  color: colors.onSurfaceVariant,
                                ),
                              ),
                              const SizedBox(width: 12),
                              CodeText(
                                line.text,
                                language: line.kind == 'header'
                                    ? null
                                    : languageFor(path),
                              ),
                            ],
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
          if (diff['truncated'] == true)
            Padding(
              padding: const EdgeInsets.all(12),
              child: Text(context.tr('resourcePartial')),
            ),
        ],
      ),
    );
  }
}
