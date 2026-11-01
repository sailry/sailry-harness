import 'package:flutter/material.dart';

import '../l10n/strings.dart';
import '../runtime/json.dart';
import '../ui/kit.dart';
import '../ui/theme.dart';
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

/// One file of a multi-file diff as a collapsible row: name, folder and
/// line counts, expanding into the unified hunks.
class DiffFile extends StatefulWidget {
  const DiffFile({
    super.key,
    required this.diff,
    this.initiallyExpanded = false,
    this.actions = const [],
  });

  final Map<String, dynamic> diff;
  final bool initiallyExpanded;

  /// Extra controls at the end of the row, such as undo.
  final List<Widget> actions;

  @override
  State<DiffFile> createState() => _DiffFileState();
}

class _DiffFileState extends State<DiffFile> {
  late bool _open = widget.initiallyExpanded;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final path = text(widget.diff['path']);
    final slash = path.lastIndexOf('/');
    final name = slash < 0 ? path : path.substring(slash + 1);
    final folder = slash < 0 ? '' : path.substring(0, slash);
    final lines = hunkLines(
      text(widget.diff['text']),
      added: widget.diff['format'] == 'added',
    );
    final added = lines.where((line) => line.kind == 'added').length;
    final removed = lines.where((line) => line.kind == 'removed').length;
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: colors.outlineVariant)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          InkWell(
            onTap: () => setState(() => _open = !_open),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 10),
              child: Row(
                children: [
                  AnimatedRotation(
                    turns: _open ? .25 : 0,
                    duration: const Duration(milliseconds: 150),
                    child: AppIcon(
                      'chevron',
                      size: 12,
                      color: colors.onSurfaceVariant,
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Text.rich(
                      TextSpan(
                        text: name,
                        children: [
                          if (folder.isNotEmpty)
                            TextSpan(
                              text: '  $folder',
                              style: TextStyle(
                                fontSize: 12,
                                color: colors.onSurfaceVariant,
                              ),
                            ),
                        ],
                      ),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 14),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    '+$added',
                    style: TextStyle(
                      fontSize: 12,
                      color: SailryTheme.statusColor(
                        context,
                        StatusTone.success,
                      ),
                    ),
                  ),
                  const SizedBox(width: 6),
                  Text(
                    '−$removed',
                    style: TextStyle(
                      fontSize: 12,
                      color: SailryTheme.statusColor(
                        context,
                        StatusTone.danger,
                      ),
                    ),
                  ),
                  ...widget.actions,
                ],
              ),
            ),
          ),
          if (_open) ...[
            ClipRRect(
              borderRadius: BorderRadius.circular(12),
              child: ColoredBox(
                color: colors.onSurface.withValues(alpha: .03),
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 420),
                  child: UnifiedDiff(diff: widget.diff),
                ),
              ),
            ),
            Row(
              children: [
                if (widget.diff['truncated'] == true)
                  Expanded(
                    child: Text(
                      context.tr('resourcePartial'),
                      style: TextStyle(
                        fontSize: 12,
                        color: colors.onSurfaceVariant,
                      ),
                    ),
                  )
                else
                  const Spacer(),
                CopyTextButton(text(widget.diff['text'])),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

/// Hunk rows for a unified view, without file metadata or newline markers.
List<DiffLine> hunkLines(String patch, {bool added = false}) => [
  for (final line in parseLines(patch, added: added))
    if (line.kind != 'header' || line.text.startsWith('@@')) line,
];

/// Unified diff rows with one line-number gutter and tinted changes.
class UnifiedDiff extends StatelessWidget {
  const UnifiedDiff({super.key, required this.diff});
  final Map<String, dynamic> diff;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final lines = hunkLines(
      text(diff['text']),
      added: diff['format'] == 'added',
    );
    final digits = lines
        .map((line) => '${line.after ?? line.before ?? ''}'.length)
        .fold(1, (width, value) => value > width ? value : width);
    final style = TextStyle(
      fontFamily: 'monospace',
      fontSize: 12.5,
      height: 1.6,
      color: colors.onSurface,
    );
    final blue = SailryTheme.accentColor(context, AccentTone.blue);
    final added = SailryTheme.statusColor(context, StatusTone.success);
    final removed = SailryTheme.statusColor(context, StatusTone.danger);
    return LayoutBuilder(
      builder: (context, constraints) => SingleChildScrollView(
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          child: ConstrainedBox(
            constraints: BoxConstraints(minWidth: constraints.maxWidth),
            child: IntrinsicWidth(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final line in lines)
                    ColoredBox(
                      color: switch (line.kind) {
                        'header' => blue.withValues(alpha: .08),
                        'added' => added.withValues(alpha: .12),
                        'removed' => removed.withValues(alpha: .12),
                        _ => Colors.transparent,
                      },
                      child: Padding(
                        padding: const EdgeInsets.symmetric(horizontal: 12),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '${line.after ?? line.before ?? ''}'.padLeft(
                                digits,
                              ),
                              style: style.copyWith(
                                color: colors.onSurfaceVariant.withValues(
                                  alpha: .55,
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                            Text(
                              line.text,
                              softWrap: false,
                              style: line.kind == 'header'
                                  ? style.copyWith(color: blue)
                                  : style,
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
      ),
    );
  }
}
